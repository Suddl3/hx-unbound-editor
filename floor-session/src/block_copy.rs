//! Duplicate one unassigned stock block in the current edit buffer.
use super::{block_move, Board, Result};
use hx_catalog::Catalog;
use hx_proto::{msgpack::Value, preset::Preset};
use hx_usb::Session;
use serde_json::Value as Json;

// These are the same preset sections traversed by hx-proto's follow_slots.
// A partial controller decode is insufficient here: an unknown assignment
// still owns its slot and must prevent the copy.
fn table_allows(table: Option<&Value>, selector: i64, position: i64, slot: usize) -> bool {
    let Some(Value::Array(groups)) = table else {
        return false;
    };
    groups.iter().all(|group| match group {
        Value::Nil => true,
        Value::Array(entries) => entries.iter().all(|entry| {
            let Some(what) = entry.get(selector) else {
                return false;
            };
            match what.get(position).and_then(Value::as_i64) {
                Some(n) => n != slot as i64,
                None => selector == 11 && what.get(0).and_then(Value::as_i64) == Some(3),
            }
        }),
        _ => false,
    })
}

fn unassigned(p: &Preset, slot: usize) -> bool {
    table_allows(p.tone.get(4), 1, 5, slot)
        && table_allows(p.tone.get(3).and_then(|s| s.get(8)), 11, 8, slot)
}

pub(super) fn available(board: &Board, cat: &Catalog, slot: usize) -> bool {
    block_move::available(board, slot)
        && board.manageable(slot, cat)
        && unassigned(&board.preset, slot)
}

fn copy_settings(p: &mut Preset, from: usize, to: usize) -> Result<()> {
    if !unassigned(p, from) || !unassigned(p, to) {
        return Err(
            "Copy requires a source and destination without controller or switch assignments"
                .into(),
        );
    }
    let raw = p.copy_slot(from).ok_or("The source block is missing")?;
    if !p.paste_slot(to, &raw) {
        return Err("This block cannot be copied into that position".into());
    }
    let count = p.slots.len();
    let section = p.tone.get_mut(10).ok_or("Missing snapshot section")?;
    // Carry the entire per-slot record (including the flag beside bypass) in
    // all snapshots, even ones the player has not recalled yet.
    let Some(Value::Array(entries)) = section.get_mut(10) else {
        return Err("Missing snapshots".into());
    };
    if entries.is_empty() {
        return Err("No snapshots available to copy".into());
    }
    for snapshot in entries {
        let Some(Value::Array(states)) = snapshot.get_mut(3) else {
            return Err("Missing snapshot block states".into());
        };
        if states.len() != count {
            return Err("Snapshot layout differs from the current path".into());
        }
        let entry = states.get(from).ok_or("Missing source state")?.clone();
        let Some(Value::Array(pair)) = states.get(to) else {
            return Err("Missing destination state".into());
        };
        if !matches!(&entry, Value::Array(fields) if fields.len()==2 && fields[1].as_bool().is_some())
            || pair.len() != 2
            || pair[1].as_bool().is_none()
        {
            return Err("Unknown per-slot snapshot data".into());
        }
        states[to] = entry;
    }
    // The section also carries a top-level flag per slot. Preserve the
    // source's policy without having to interpret the flag's polarity.
    if let Some(policy) = section.get_mut(13) {
        match policy {
            Value::Array(flags)
                if flags.len() == count && flags.iter().all(|x| x.as_bool().is_some()) =>
            {
                flags[to] = flags[from].clone();
            }
            _ => return Err("Unknown per-slot snapshot policy".into()),
        }
    }
    Ok(())
}

fn prepare(p: &Preset, from: usize, to: usize) -> Result<Preset> {
    let mut copy = Preset::parse(&p.encode()).ok_or("Invalid current preset")?;
    copy_settings(&mut copy, from, to)?;
    let encoded = copy.encode();
    let mut checked = Preset::parse(&encoded).ok_or("The prepared copy did not round-trip")?;
    // write_preset normalises each empty branch first. Refuse a layout in
    // which that would also edit something outside the requested copy.
    checked.settle_branches();
    if checked.encode() != encoded {
        return Err("This routing requires a branch edit to copy a block".into());
    }
    Ok(copy)
}

pub(super) fn execute(
    session: &mut Session,
    board: &mut Board,
    cat: &Catalog,
    cmd: &Json,
) -> Result<String> {
    let from = block_move::position(cmd, "slot")?;
    let to = block_move::position(cmd, "to")?;
    let identity = board.identity.clone();
    let before = Board::read(session)?;
    let model = before
        .preset
        .slots
        .get(from)
        .and_then(|s| s.model)
        .ok_or("Empty source position")?;
    if identity != before.identity
        || cmd["model"].as_u64() != Some(model as u64)
        || before.preset.slots != board.preset.slots
        || !available(&before, cat, from)
        || !before.empty_cell(to)
    {
        return Err(
            "Choose a current unassigned stock block and an empty position on straight path 1A"
                .into(),
        );
    }
    // Validate the entire clone before requesting even an unsaved placement.
    prepare(&before.preset, from, to)?;
    if session.preset_info()? != identity {
        return Err("Preset changed before copy".into());
    }

    // Let the device allocate its own DSP before copying configured settings.
    // This single native placement avoids bypassing the current path's budget.
    let pair = before.preset.slots[from].paired;
    let allocation = match pair {
        Some(cab) => session.set_model_pair(to as i64, model, cab),
        None => session.set_model(to as i64, model),
    };
    *board = Board::read(session)?;
    if allocation.is_err()
        || identity != board.identity
        || board
            .preset
            .slots
            .get(to)
            .is_none_or(|s| s.model != Some(model) || s.paired != pair)
    {
        return Err("The Floor did not confirm the new block. Check the displayed path.".into());
    }
    if board.preset.copy_slot(from) != before.preset.copy_slot(from)
        || !available(board, cat, from)
        || board.preset.tone.get(10).and_then(|s| s.get(6))
            != before.preset.tone.get(10).and_then(|s| s.get(6))
    {
        return Err(
            "The source or snapshot changed. The new block may retain its defaults.".into(),
        );
    }
    let staged = board.preset.encode();
    let configured = prepare(&board.preset, from, to)?;
    if session.read_preset()?.encode() != staged || session.preset_info()? != identity {
        return Err(
            "The edit buffer changed after placement. Check the new block before continuing."
                .into(),
        );
    }
    if configured.encode() != staged {
        session.write_preset(&configured)?;
    }
    *board = Board::read(session)?;
    if identity != board.identity
        || board.preset.copy_slot(to) != configured.copy_slot(to)
        || !block_move::matches_move(&configured, &board.preset)
    {
        return Err("The copy could not be confirmed. Check the current path and settings.".into());
    }
    Ok(format!(
        "Copied settings from 1A {from} to {to} · Floor confirmed"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn preset() -> Preset {
        let deps=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../vendor/tonepush")
            .join("crates/hx-proto/tests/fixtures/gen-02-one-drive.hxpreset");
        Preset::parse(&std::fs::read(deps).unwrap()).unwrap()
    }
    #[test]
    fn copy_carries_settings_and_every_snapshot() {
        let mut p = preset();
        let source = p.copy_slot(1).unwrap();
        let Some(Value::Array(entries)) = p.tone.at_mut(&[10, 10]) else {
            panic!()
        };
        for (i, snap) in entries.iter_mut().enumerate() {
            let Some(Value::Array(states)) = snap.get_mut(3) else {
                panic!()
            };
            states[1] = Value::Array(vec![Value::Bool(false), Value::Bool(i % 2 == 0)]);
        }
        let Some(Value::Array(flags)) = p.tone.at_mut(&[10, 13]) else {
            panic!()
        };
        flags[1] = Value::Bool(false);
        copy_settings(&mut p, 1, 4).unwrap();
        let back = Preset::parse(&p.encode()).unwrap();
        assert_eq!(back.copy_slot(1), Some(source.clone()));
        assert_eq!(back.copy_slot(4), Some(source));
        for (i, s) in back.snapshot_details().iter().enumerate() {
            assert_eq!(s.enabled[4], Some(i % 2 == 0));
        }
        let Some(Value::Array(flags)) = back.tone.get(10).and_then(|s| s.get(13)) else {
            panic!()
        };
        assert_eq!(flags[4].as_bool(), Some(false));
    }
    #[test]
    fn assignments_on_either_end_prevent_copy() {
        for slot in [1, 4] {
            let mut p = preset();
            let Some(Value::Array(groups)) = p.tone.get_mut(4) else {
                panic!()
            };
            groups[0] = Value::Array(vec![
                hx_proto::msgmap! {1 => hx_proto::msgmap!{5 => Value::Int(slot)}},
            ]);
            let before = p.encode();
            assert!(copy_settings(&mut p, 1, 4).is_err());
            assert_eq!(p.encode(), before);
        }
    }

    #[test]
    #[ignore = "requires the local 07C Floor-only USBPcap recording"]
    fn copy_on_original_floor_snapshot_layout() {
        // Reassemble the inbound preset in frames 3045 onward. This keeps the
        // player's recording local and makes the exact hardware fixture reproducible.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../build/hx-edit-capture/floor-20261106-150852.pcap");
        let pcap = std::fs::read(path).unwrap();
        let mut offset = 24;
        let mut blob = Vec::new();
        for number in 1..=3400 {
            let length =
                u32::from_le_bytes(pcap[offset + 8..offset + 12].try_into().unwrap()) as usize;
            let pkt = &pcap[offset + 16..offset + 16 + length];
            offset += 16 + length;
            if number < 3045 {
                continue;
            }
            let header = u16::from_le_bytes(pkt[..2].try_into().unwrap()) as usize;
            let data = &pkt[header..];
            if pkt[21] != 0x81 || data.len() < 17 || data[4..8] != [0xed, 3, 3, 0x10] {
                continue;
            }
            let used = 8 + (u32::from_le_bytes([data[0], data[1], data[2], 0]) as usize);
            if number == 3045 {
                let start = data.windows(9).position(|s| s == b"\xa9l6-helix").unwrap();
                assert_eq!(&data[start - 3..start], &[0xda, 0x19, 0x8f]);
                blob.extend_from_slice(&data[start..used]);
            } else {
                blob.extend_from_slice(&data[16..used]);
            }
            if blob.len() >= 0x198f {
                break;
            }
        }
        let mut actual = Preset::parse(&blob[..0x198f]).expect("original Floor edit buffer");
        assert_eq!(actual.firmware().as_deref(), Some("3.80"));
        assert_eq!(actual.snapshots().len(), 8);
        assert_eq!(actual.layout().paths.len(), 2);
        let stock = preset().copy_slot(1).unwrap();
        assert!(actual.paste_slot(1, &stock)); // offline stand-in for a stock model placement
        copy_settings(&mut actual, 1, 4).unwrap();
        let copied = Preset::parse(&actual.encode()).unwrap();
        assert_eq!(copied.copy_slot(4), Some(stock));
        assert!(copied
            .snapshot_details()
            .iter()
            .all(|s| s.enabled[4] == s.enabled[1]));
    }
}
