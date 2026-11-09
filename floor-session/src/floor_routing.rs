//! Read both DSPs' primary endpoints and junctions from the native edit buffer.
//! Opcode 42 addresses a slot without specifying a DSP. Writes are limited to
//! the first DSP, where the native and flattened slot numbers agree.
use super::{Board, Catalog, Json, Kind, Result, Session};
use hx_proto::preset::{Layout, Path, Preset};
use serde_json::json;

#[derive(Clone, Copy)]
enum Endpoint {
    Input,
    Output,
}

impl Endpoint {
    fn slot(self, path: &Path) -> Option<usize> {
        match self {
            Self::Input => path.input,
            Self::Output => path.output,
        }
    }

    fn menu(self, dsp: usize, cat: &Catalog) -> Option<&[String]> {
        let (symbol, field, display) = match self {
            Self::Input if dsp == 0 => ("HD2_AppDSPFlow1Input", "@input", "input_type"),
            Self::Input => ("HD2_AppDSPFlow2Input", "@input", "input_type"),
            Self::Output => ("HD2_AppDSPFlowOutput", "@output", "output_type"),
        };
        let param = cat.model(symbol)?.params.iter().find(|p| p.id == field)?;
        let menu = cat.choices(param)?;
        (param.kind == Kind::Enum
            && param.display.as_deref() == Some(display)
            && param.min == 0.0
            && param.max + 1.0 == menu.len() as f32)
            .then_some(menu)
    }

    fn view(self, preset: &Preset, cat: &Catalog, layout: &Layout, dsp: usize) -> Option<Json> {
        let slot = self.slot(layout.paths.get(dsp)?)?;
        let route = preset.routing(slot);
        let menu = self.menu(dsp, cat);
        let name = route
            .and_then(|n| usize::try_from(n).ok())
            .and_then(|i| menu?.get(i));
        let expected_slot = match self {
            Self::Input => 0,
            Self::Output => 9,
        };
        let writable = dsp == 0
            && slot == expected_slot
            && layout.paths.len() == 2
            && preset.firmware().as_deref() == Some("3.80")
            && preset.slot_address(slot) == Some((0, slot))
            && name.is_some();
        Some(json!({"slot":slot,"value":route,"name":name,
                    "choices":if writable {menu} else {None},"writable":writable}))
    }
}

fn junction(
    preset: &Preset,
    cat: &Catalog,
    layout: &Layout,
    dsp: usize,
    slot: Option<usize>,
) -> Option<Json> {
    let slot = slot?;
    let path = layout.paths.get(dsp)?;
    let model = preset.junction_model(slot);
    let name = model.and_then(|id| cat.model_number(id)).map(|m| &m.name);
    let at = preset.attach_of(slot);
    let column = at.and_then(|s| {
        if Some(s) == path.input {
            return Some(0);
        }
        if Some(s) == path.output {
            return Some(9);
        }
        let (branch, pos) = layout.position_of(dsp, s)?;
        (preset.slot_address(s)?.0 == dsp && branch == 0 && pos < 8).then_some(pos + 1)
    });
    Some(
        json!({"slot":slot,"model":model,"name":name,"column":column,
                "attach":preset.local_attach_of(slot)}),
    )
}

pub(super) fn paths(preset: &Preset, layout: &Layout, cat: &Catalog) -> Vec<Json> {
    layout
        .paths
        .iter()
        .enumerate()
        .map(|(dsp, path)| {
            let lower = path
                .lanes
                .iter()
                .any(|l| l.branch == 1 && !l.blocks.is_empty());
            json!({"split":lower, "input":Endpoint::Input.view(preset,cat,layout,dsp),
               "output":Endpoint::Output.view(preset,cat,layout,dsp),
               "fork":junction(preset,cat,layout,dsp,path.split),
               "merge":junction(preset,cat,layout,dsp,path.join)})
        })
        .collect()
}

fn target(preset: &Preset, cat: &Catalog, cmd: &Json) -> Result<(usize, i64, String)> {
    let endpoint = match cmd["endpoint"].as_str() {
        Some("input") => Endpoint::Input,
        Some("output") => Endpoint::Output,
        _ => return Err("Choose a path endpoint".into()),
    };
    let layout = preset.layout();
    let info = endpoint
        .view(preset, cat, &layout, 0)
        .ok_or("Path endpoint unavailable")?;
    let slot = info["slot"].as_u64().ok_or("Missing endpoint slot")? as usize;
    if cmd["dsp"].as_u64() != Some(0)
        || info["writable"] != true
        || cmd["slot"] != slot
        || cmd["expected"].as_i64() != info["value"].as_i64()
    {
        return Err("The path routing changed. Review the latest input and output.".into());
    }
    let next = cmd["value"]
        .as_i64()
        .ok_or("Choose a Floor routing destination")?;
    let name = usize::try_from(next)
        .ok()
        .and_then(|i| info["choices"].get(i))
        .and_then(Json::as_str)
        .ok_or("Unknown Floor routing destination")?;
    Ok((slot, next, name.to_owned()))
}

pub(super) fn edit(
    session: &mut Session,
    board: &mut Board,
    cat: &Catalog,
    cmd: &Json,
) -> Result<String> {
    if session.profile.product_id != 0x4248 {
        return Err("Routing requires a Helix Floor".into());
    }
    let identity = board.identity.clone();
    *board = Board::read(session)?;
    let (slot, next, name) = target(&board.preset, cat, cmd)?;
    if identity != board.identity || session.preset_info()? != identity {
        return Err("The Floor changed presets before routing".into());
    }
    if board.preset.routing(slot) != Some(next) {
        session.set_routing(slot as i64, next)?;
        *board = Board::read(session)?;
    }
    if board.identity != identity || board.preset.routing(slot) != Some(next) {
        return Err("Endpoint readback differs. Review the current routing.".into());
    }
    Ok(format!(
        "Floor confirms {} ÃƒÆ’Ã†â€™Ãƒâ€ Ã¢â‚¬â„¢ÃƒÆ’Ã¢â‚¬Â ÃƒÂ¢Ã¢â€šÂ¬Ã¢â€žÂ¢ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã…Â¡Ãƒâ€šÃ‚Â¬ÃƒÆ’Ã¢â‚¬Â¦Ãƒâ€šÃ‚Â¡ÃƒÆ’Ã†â€™Ãƒâ€ Ã¢â‚¬â„¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€¦Ã‚Â¡ÃƒÆ’Ã†â€™ÃƒÂ¢Ã¢â€šÂ¬Ã…Â¡ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â· {name}",
        cmd["endpoint"].as_str().unwrap_or("routing")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn captured() -> Preset {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../build/hx-edit-capture/recorded-floor-edit-buffer.hxpreset");
        Preset::parse(&std::fs::read(file).expect("recorded Floor edit buffer")).unwrap()
    }
    #[test]
    #[ignore = "requires a private recorded Floor edit buffer, not distributed"]
    fn original_floor_has_multi_none_multi_and_dormant_junctions() {
        let preset = captured();
        let views = paths(&preset, &preset.layout(), &super::super::catalog().unwrap());
        assert_eq!(views.len(), 2);
        assert_eq!(views[0]["input"]["choices"][14], "USB 3/4");
        assert_eq!(views[0]["input"]["value"], 1);
        assert_eq!(views[0]["input"]["name"], "Multi (Guitar, Aux, Variax)");
        assert_eq!(views[0]["output"]["choices"][2], "Path 2A");
        assert_eq!(views[1]["input"]["name"], "None");
        assert_eq!(views[1]["output"]["value"], 1);
        assert!(views[1]["output"]["choices"].is_null());
        assert_eq!(views[0]["split"], false);
        assert_eq!(views[1]["fork"]["name"], "Split Y");
        assert_eq!(views[1]["merge"]["name"], "Mixer");
        assert_eq!(views[1]["fork"]["column"], 0);
    }
    #[test]
    #[ignore = "requires a private recorded Floor edit buffer, not distributed"]
    fn native_route_edit_requires_a_known_current_first_path_endpoint() {
        let mut preset = captured();
        let cat = super::super::catalog().unwrap();
        let request = json!({"dsp":0,"endpoint":"input","slot":0,"expected":1,"value":14});
        let (slot, next, name) = target(&preset, &cat, &request).unwrap();
        assert_eq!(name, "USB 3/4");
        assert!(preset.set_routing(slot, next));
        assert!(target(&preset, &cat, &request).is_err());
        let parsed = Preset::parse(&preset.encode()).unwrap();
        assert_eq!(parsed.routing(0), Some(14));
        assert_eq!(parsed.routing(20), Some(0));
        for (field, bad) in [
            ("dsp", json!(1)),
            ("slot", json!(20)),
            ("endpoint", json!("output")),
            ("value", json!(17)),
            ("value", json!(-1)),
            ("value", json!("1")),
        ] {
            let mut cmd = request.clone();
            cmd["expected"] = json!(14);
            cmd[field] = bad;
            assert!(target(&parsed, &cat, &cmd).is_err(), "accepted {cmd}");
        }
        assert!(preset.set_routing(0, 700));
        let unknown = paths(&preset, &preset.layout(), &cat);
        assert_eq!(unknown[0]["input"]["value"], 700);
        assert_eq!(unknown[0]["input"]["writable"], false);
    }

    #[test]
    #[ignore = "requires a private recorded Floor edit buffer, not distributed"]
    fn occupied_lower_branch_uses_its_own_dsps_junctions() {
        let mut preset = captured();
        preset.slots[32].model = Some(388);
        preset.slots[32].kind = hx_proto::preset::Kind::Block;
        let layout = preset.layout();
        assert_eq!(layout.position_of(1, 32), Some((1, 1)));
        let result = paths(&preset, &layout, &super::super::catalog().unwrap());
        assert_eq!(result[0]["split"], false);
        assert_eq!(result[1]["split"], true);
        assert_eq!(result[1]["fork"]["slot"], 30);
        assert_eq!(result[1]["merge"]["slot"], 39);
    }
}
