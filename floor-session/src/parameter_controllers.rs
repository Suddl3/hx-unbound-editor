//! Direct parameter-controller assignment on the original Floor. A single edit
//! changes one source or one CC. The full preset supplies the assignment range.
use super::{nam_parameter_wire, nam_selector, Board, Catalog, Json, Kind, Result, Session};
use hx_proto::{
    msgpack::Value,
    preset::{Assignment, Preset, Target},
    rpc::{ControlLayout, Source},
};
use serde_json::json;

// The preset stores a switch's carried blocks in a separate table from the
// parameter controller records. A link on a switch that also drives a knob
// on that block cannot be identified as bypass by block number alone.
fn switch_links(preset: &Preset, layout: ControlLayout, rows: &[Json]) -> Option<Vec<Json>> {
    let Value::Array(switches) = preset.tone.get(3)?.get(8)? else {
        return None;
    };
    let mut result = Vec::new();
    for source in layout.sources() {
        let Source::Footswitch(number) = source else {
            continue;
        };
        let ordinal = layout.ordinal(source)?;
        let entries = match switches.get(usize::try_from(ordinal).ok()?) {
            None | Some(Value::Nil) => continue,
            Some(Value::Array(entries)) => entries,
            _ => return None,
        };
        for entry in entries {
            let target = entry.get(11)?;
            match target.get(0)?.as_i64()? {
                3 => continue, // A switch command without a block.
                1 | 2 => {}
                _ => return None, // Unrecognized target, refuse switch writes.
            }
            let slot = target.get(8)?.as_i64()?;
            if !(0..preset.slots.len() as i64).contains(&slot) {
                return None;
            }
            let shares_param = rows
                .iter()
                .any(|r| r["slot"] == slot && r["source"] == ordinal && r["target"] != "bypass");
            result.push(json!({"slot":slot,"switch":number,"sharedParameter":shares_param}));
        }
    }
    Some(result)
}

fn row(assignment: &Assignment, layout: ControlLayout) -> Json {
    let (target, index) = match assignment.target {
        Target::Param(index) => ("parameter", Some(index)),
        Target::Bypass => ("bypass", None),
        Target::Mic => ("mic", None),
    };
    json!({"slot":assignment.block,"target":target,"index":index,
        "source":layout.ordinal(assignment.source),"name":assignment.source.label(),
        "cc":assignment.cc,"min":assignment.min,"max":assignment.max})
}

pub(super) fn view(preset: &Preset, layout: ControlLayout) -> Json {
    let rows: Vec<_> = preset
        .assignments(layout)
        .iter()
        .map(|a| row(a, layout))
        .collect();
    let sources: Vec<_> = layout.sources().iter()
        .map(|s| json!({"value":layout.ordinal(*s),"name":s.label(),"midi":*s==Source::MidiCc,
            "snapshot":*s==Source::Snapshots,"switch":match *s {Source::Footswitch(n)=>Some(n),_=>None}}))
        .collect();
    json!({"rows":rows,"sources":sources,"switchLinks":switch_links(preset,layout,&rows)})
}

fn target_rows(preset: &Preset, layout: ControlLayout, slot: usize, index: usize) -> Vec<Json> {
    preset
        .assignments(layout)
        .iter()
        .filter(|a| a.block == slot as i64 && a.target == Target::Param(index as i64))
        .map(|a| row(a, layout))
        .collect()
}

#[derive(Debug, PartialEq, Copy, Clone)]
enum Change {
    Source(Option<Source>),
    MidiCc(i64),
    Range { high: bool, value: f32 },
}

fn change(cmd: &Json, layout: ControlLayout, before: &[Json]) -> Result<Change> {
    let expected = cmd["expected"]
        .as_array()
        .ok_or("Missing assignment context")?;
    // Browser JSON serializes 1.0 as 1; compare range ends numerically.
    let unchanged = expected.len() == before.len()
        && expected.iter().zip(before).all(|(old, current)| {
            ["slot", "target", "index", "source", "cc"]
                .iter()
                .all(|key| old[*key] == current[*key])
                && ["min", "max"]
                    .iter()
                    .all(|key| old[*key].as_f64() == current[*key].as_f64())
        });
    if !unchanged || before.len() > 1 {
        return Err(
            "Controller assignment changed or has multiple sources. Review the Floor state.".into(),
        );
    }
    let actual = before.first();
    match cmd["field"].as_str() {
        Some("source") => {
            let ordinal = cmd["value"].as_i64().ok_or("Choose a controller source")?;
            let source = if ordinal == Source::NONE {
                None
            } else {
                let supported = layout
                    .source(ordinal)
                    .filter(|s| layout.sources().contains(s))
                    .ok_or("Unsupported controller source")?;
                Some(supported)
            };
            if actual.and_then(|a| a["source"].as_i64()).unwrap_or(0) == ordinal {
                return Err("That source is already selected".into());
            }
            Ok(Change::Source(source))
        }
        Some("cc")
            if actual.and_then(|a| a["source"].as_i64()) == layout.ordinal(Source::MidiCc) =>
        {
            let number = cmd["value"]
                .as_i64()
                .filter(|n| (0..=127).contains(n))
                .ok_or("Choose a MIDI CC from 0 to 127")?;
            Ok(Change::MidiCc(number))
        }
        Some(field @ ("min" | "max")) if actual.is_some() => {
            let value = cmd["value"]
                .as_f64()
                .filter(|v| (0.0..=1.0).contains(v))
                .ok_or("Choose a range endpoint from 0 to 100 percent")?
                as f32;
            Ok(Change::Range {
                high: field == "max",
                value,
            })
        }
        _ => Err("Invalid controller edit".into()),
    }
}

impl Change {
    fn matches(&self, after: &[Json], layout: ControlLayout) -> bool {
        match self {
            Self::Source(None) => after.is_empty(),
            Self::Source(Some(source)) => {
                after.len() == 1 && after[0]["source"].as_i64() == layout.ordinal(*source)
            }
            Self::MidiCc(cc) => {
                after.len() == 1
                    && after[0]["source"].as_i64() == layout.ordinal(Source::MidiCc)
                    && after[0]["cc"].as_i64() == Some(*cc)
            }
            Self::Range { high, value } => {
                after.len() == 1
                    && after[0][if *high { "max" } else { "min" }]
                        .as_f64()
                        .is_some_and(|v| (v - f64::from(*value)).abs() < 0.0001)
            }
        }
    }
}

pub(super) fn edit(
    session: &mut Session,
    board: &mut Board,
    cat: &Catalog,
    cmd: &Json,
) -> Result<String> {
    let slot = cmd["slot"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or("Invalid block")?;
    let identity = board.identity.clone();
    *board = Board::read(session)?;
    let layout = session.profile.controls;
    if identity != board.identity || layout != ControlLayout::Helix || !board.top_cell(slot) {
        return Err("Choose a parameter on path 1A of the current Floor preset".into());
    }
    let block = &board.preset.slots[slot];
    let model = block.model.ok_or("Empty block")?;
    if cmd["model"].as_u64() != Some(model.into()) {
        return Err("The block model changed".into());
    }
    if cmd["op"] == "controller-bypass" {
        if block.paired.is_some()
            || !(board.editable(slot, cat) || nam_selector(&board.preset, slot).is_some())
        {
            return Err("Bypass assignment is unavailable for this block".into());
        }
        return bypass_edit(session, board, slot, model, cmd);
    }
    let index = cmd["index"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or("Invalid parameter")?;
    let value = *block.values.get(index).ok_or("Parameter unavailable")?;
    let supported = model != 380 && board.editable(slot, cat)
        && cat
            .param(model, index)
            .is_some_and(|p| p.accepts(value) && p.max > p.min && p.kind != Kind::Text);
    let nam_control = model == 380
        && ((index == 0 && nam_selector(&board.preset, slot).is_some())
            || nam_parameter_wire(&board.preset, slot, index, value).is_some());
    if !(supported || nam_control) {
        return Err("The block changed or this parameter is read-only".into());
    }
    let before = target_rows(&board.preset, layout, slot, index);
    let change = change(cmd, layout, &before)?;
    if matches!(change, Change::Range { .. }) {
        let normalized = if model == 380 {
            (1..=6).contains(&index)
        } else {
            cat.param(model, index)
                .is_some_and(|p| p.min == 0.0 && p.max == 1.0 && p.kind == Kind::Continuous)
        };
        if !normalized || before[0]["source"].as_i64() == layout.ordinal(Source::Snapshots) {
            return Err("Range editing requires a normalized continuous parameter and a physical controller".into());
        }
    }
    // Verify that the Floor also recognizes the target, including an empty
    // assignment. Opcode 36 reports the source but its ranges can be stale.
    let device_source = session
        .read_assignment(slot as i64, index as i64)?
        .map(|a| a.source);
    if device_source.and_then(|s| layout.ordinal(s))
        != before.first().and_then(|a| a["source"].as_i64())
        || session.preset_info()? != identity
    {
        return Err("The Floor changed before the controller edit".into());
    }
    match change {
        Change::Source(source) => session.assign_parameter(slot as i64, index as i64, source)?,
        Change::MidiCc(cc) => session.set_assign_cc(slot as i64, index as i64, cc)?,
        Change::Range { high, value } => {
            session.set_assign_range(slot as i64, index as i64, value, high)?
        }
    }
    *board = Board::read(session)?;
    let after = target_rows(&board.preset, layout, slot, index);
    let same_source = !matches!(change, Change::Range { .. })
        || before.first().and_then(|a| a["source"].as_i64())
            == after.first().and_then(|a| a["source"].as_i64());
    if board.identity != identity
        || board.preset.slots.get(slot).and_then(|s| s.model) != Some(model)
        || !same_source
        || !change.matches(&after, layout)
    {
        return Err("Controller change unconfirmed. Review the current preset.".into());
    }
    Ok("Controller assignment confirmed by Floor".into())
}

fn bypass_state(links: &[Json], slot: usize, switch: u8) -> Result<bool> {
    let matching: Vec<_> = links
        .iter()
        .filter(|r| r["slot"] == slot && r["switch"] == switch)
        .collect();
    match matching.as_slice() {
        [] => Ok(false),
        [one] if one["sharedParameter"] == false => Ok(true),
        _ => Err("That switch also carries a parameter or multiple entries for this block".into()),
    }
}

fn bypass_edit(
    session: &mut Session,
    board: &mut Board,
    slot: usize,
    model: u32,
    cmd: &Json,
) -> Result<String> {
    let switch = cmd["switch"]
        .as_u64()
        .and_then(|n| u8::try_from(n).ok())
        .filter(|n| {
            session
                .profile
                .controls
                .sources()
                .contains(&Source::Footswitch(*n))
        })
        .ok_or("Choose an available Floor footswitch")?;
    let enable = cmd["value"].as_bool().ok_or("Choose assign or remove")?;
    let identity = board.identity.clone();
    let read_links = |p: &Preset, layout| -> Result<Vec<Json>> {
        let rows: Vec<_> = p
            .assignments(layout)
            .iter()
            .map(|a| row(a, layout))
            .collect();
        switch_links(p, layout, &rows).ok_or_else(|| "Unrecognized footswitch assignments".into())
    };
    let links = read_links(&board.preset, session.profile.controls)?;
    let existing: Vec<_> = links
        .iter()
        .filter(|r| r["slot"] == slot)
        .cloned()
        .collect();
    if cmd["expected"].as_array() != Some(&existing)
        || bypass_state(&links, slot, switch)? == enable
    {
        return Err("The switch assignment changed. Review the current block.".into());
    }
    if enable && !existing.is_empty() {
        return Err("Review this block’s existing switch links before choosing another.".into());
    }
    let switch_carries = |session: &mut Session| -> Result<bool> {
        Ok(session
            .read_switch(switch)?
            .carries
            .iter()
            .any(|c| c.block == slot as i64))
    };
    if switch_carries(session)? == enable || session.preset_info()? != identity {
        return Err("Footswitch readback changed before the edit".into());
    }
    if enable {
        session.assign_bypass_footswitch(slot as i64, switch)?;
    } else {
        session.unassign_bypass_footswitch(slot as i64, switch)?;
    }
    *board = Board::read(session)?;
    let after = read_links(&board.preset, session.profile.controls)?;
    if board.identity != identity
        || board.preset.slots.get(slot).and_then(|b| b.model) != Some(model)
        || bypass_state(&after, slot, switch)? != enable
        || switch_carries(session)? != enable
    {
        return Err("Bypass assignment unconfirmed. Review the Floor.".into());
    }
    Ok(format!("FS{switch} bypass assignment confirmed by Floor"))
}

#[cfg(test)]
mod tests {
    use super::*;
    const FLOOR: ControlLayout = ControlLayout::Helix;

    #[test]
    fn snapshots_are_floor_ordinal_19_and_exp_and_switches_are_device_specific() {
        let request = |n| json!({"expected":[],"field":"source","value":n});
        for (n, source) in [
            (19, Source::Snapshots),
            (3, Source::Expression(3)),
            (16, Source::Footswitch(11)),
            (18, Source::MidiCc),
        ] {
            assert_eq!(
                change(&request(n), FLOOR, &[]).unwrap(),
                Change::Source(Some(source))
            );
        }
        for n in [-1, 0, 17, 20] {
            assert!(change(&request(n), FLOOR, &[]).is_err());
        }
    }

    #[test]
    fn cc_and_remove_require_matching_readback() {
        let before = vec![json!({"source":18,"cc":20,"min":1,"max":128})];
        let cmd = json!({"expected":before,"field":"cc","value":21});
        let edit = change(&cmd, FLOOR, &before).unwrap();
        assert!(!edit.matches(&before, FLOOR));
        assert!(edit.matches(&[json!({"source":18,"cc":21})], FLOOR));
        assert!(!edit.matches(&[json!({"source":19,"cc":21})], FLOOR));
        assert!(change(&cmd, FLOOR, &[]).is_err());
        let off = change(
            &json!({"expected":before,"field":"source","value":0}),
            FLOOR,
            &before,
        )
        .unwrap();
        assert!(off.matches(&[], FLOOR));
        assert!(!off.matches(&before, FLOOR));
    }

    #[test]
    fn assignment_range_compares_numeric_readbacks_and_rejects_wrong_end() {
        let before = vec![json!({"source":1,"cc":null,"min":0.0,"max":1.0})];
        let expected = vec![json!({"source":1,"cc":null,"min":0,"max":1})];
        let change = change(
            &json!({"expected":expected,"field":"max","value":0.4}),
            FLOOR,
            &before,
        )
        .unwrap();
        assert!(change.matches(&[json!({"min":0,"max":0.4})], FLOOR));
        assert!(!change.matches(&[json!({"min":0.4,"max":1})], FLOOR));
    }

    #[test]
    fn a_shared_parameter_cannot_be_removed_as_a_bypass_link() {
        let link = json!({"slot":4,"switch":3,"sharedParameter":false});
        assert!(bypass_state(&[link.clone()], 4, 3).unwrap());
        assert!(!bypass_state(&[link.clone()], 1, 3).unwrap());
        assert!(bypass_state(&[link.clone(), link], 4, 3).is_err());
        assert!(
            bypass_state(&[json!({"slot":4,"switch":3,"sharedParameter":true})], 4, 3).is_err()
        );
    }
}
