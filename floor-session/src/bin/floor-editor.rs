use hx_catalog::{Catalog, Kind};
#[path = "../nam_storage.rs"]
mod nam_storage;
#[path = "../snapshots.rs"]
mod snapshots;
#[path = "../parameter_controllers.rs"]
mod parameter_controllers;
#[path = "../block_move.rs"]
mod block_move;
#[path = "../block_copy.rs"]
mod block_copy;
#[path = "../preset_save.rs"]
mod preset_save;
#[path = "../configured_blocks.rs"]
mod configured_blocks;
#[path = "../backup_view.rs"]
mod backup_view;
#[path = "../floor_routing.rs"]
mod floor_routing;
#[path = "../event_sync.rs"]
mod event_sync;
use hx_proto::{msgpack::Value, preset::Preset};
use hx_usb::Session;
use serde_json::{json, Value as Json};
use std::{
    error::Error,
    io::{self, BufRead, Read},
    path::PathBuf,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Identity = (i64, i64, String);

fn stock_choice(cat: &Catalog, id: u32) -> Option<(&hx_catalog::Model, &hx_catalog::Category)> {
    let model = cat.model_number(id)?;
    let category = cat.category(cat.category_of(&model.id)?)?;
    // These categories place a single stock model. Custom NAM uses ID 380.
    let single_cab = category.id == hx_catalog::Category::CAB
        && category.subcategories.iter().any(|s| {
            matches!(s.name.as_str(), "Single" | "Single Legacy") && s.models.contains(&model.id)
        })
        && cat.symbol(id).is_some_and(|s| s.symbol == model.id);
    (id != 380 && (matches!(category.id, 1..=9 | 11 | 12 | 15) || single_cab))
        .then_some((model, category))
}

fn placement_pair(cat: &Catalog, model: u32) -> Option<u32> {
    let (amp, category) = stock_choice(cat, model)?;
    if category.id != hx_catalog::Category::AMP {
        return None;
    }
    let cab = cat.paired_cab(amp)?;
    let mut matches = cat.symbols().iter().filter(|s| s.symbol == cab.id);
    let id = matches.next()?.number;
    if matches.next().is_some() {
        return None;
    }
    let (_, category) = stock_choice(cat, id)?;
    (category.id == hx_catalog::Category::CAB).then_some(id)
}

fn stock_picker(cat: &Catalog) -> Vec<Json> {
    let mut rows = Vec::new();
    for symbol in cat.symbols() {
        if let Some((model, category)) = stock_choice(cat, symbol.number) {
            let variant = if symbol.symbol.ends_with("Stereo") {
                "Stereo"
            } else if symbol.symbol.ends_with("Mono") {
                "Mono"
            } else {
                ""
            };
            rows.push(json!({"model":symbol.number,"name":model.name,"category":category.name,"variant":variant,
                "symbol":symbol.symbol,"defaultCab":placement_pair(cat,symbol.number)}));
        }
    }
    rows.sort_by(|a, b| {
        (
            a["category"].as_str(),
            a["name"].as_str(),
            a["variant"].as_str(),
        )
            .cmp(&(
                b["category"].as_str(),
                b["name"].as_str(),
                b["variant"].as_str(),
            ))
    });
    rows
}

fn catalog() -> Result<Catalog> {
    let program_files = std::env::var_os("ProgramFiles(x86)")
        .ok_or("Windows installation directory unavailable")?;
    Ok(Catalog::load_from(
        &PathBuf::from(program_files).join("Line6/HX Edit/res"),
    )?)
}

struct Board {
    identity: Identity,
    preset: Preset,
    irs: Vec<(i64, String)>,
}

impl Board {
    fn top_cell(&self, slot: usize) -> bool {
        (1..=8).contains(&slot)
            && self.preset.firmware().as_deref() == Some("3.80")
            && self.preset.slot_address(slot) == Some((0, slot))
            && self.preset.layout().position_of(0, slot) == Some((0, slot - 1))
    }

    fn empty_cell(&self, slot: usize) -> bool {
        self.top_cell(slot)
            && self.preset.slots.get(slot).is_some_and(|b| {
                matches!(
                    b.kind,
                    hx_proto::preset::Kind::Block | hx_proto::preset::Kind::Empty
                ) && b.model.is_none()
                    && b.paired.is_none()
            })
    }

    fn manageable(&self, slot: usize, cat: &Catalog) -> bool {
        if !self.top_cell(slot) {
            return false;
        }
        let block = &self.preset.slots[slot];
        let Some(model) = block.model else {
            return false;
        };
        if block.kind != hx_proto::preset::Kind::Block || stock_choice(cat, model).is_none() {
            return false;
        }
        block.paired.is_none() || block.paired == placement_pair(cat, model)
    }
    fn read(session: &mut Session) -> Result<Self> {
        for _ in 0..3 {
            let identity = session.preset_info()?;
            let preset = session.read_preset()?;
            let irs = session.irs()?;
            if session.preset_info()? == identity {
                return Ok(Self {
                    identity,
                    preset,
                    irs,
                });
            }
        }
        Err("Floor preset changed during read. Reconnect when ready.".into())
    }

    fn editable(&self, slot: usize, cat: &Catalog) -> bool {
        let Some((_, block)) = self.preset.blocks().find(|(pos, _)| *pos == slot) else {
            return false;
        };
        let Some(model) = block.model else {
            return false;
        };
        if model==380 {
            return self.top_cell(slot) && nam_selector(&self.preset,slot).is_some();
        }
        self.preset.firmware().as_deref() == Some("3.80")
            && block.paired.is_none()
            && self.preset.slot_address(slot) == Some((0, slot))
            && cat
                .model_number(model)
                .and_then(|m| cat.category_of(&m.id))
                .and_then(|id| cat.category(id))
                .is_some_and(|c| c.is_effect() || stock_choice(cat,model).is_some())
    }

    fn view(&self, profile: hx_proto::DeviceProfile, cat: &Catalog, nam_ready: bool) -> Json {
        let layout = self.preset.layout();
        let mut blocks = Vec::new();
        for (slot, block) in self.preset.slots.iter().enumerate().filter(|(_, b)| {
            matches!(
                b.kind,
                hx_proto::preset::Kind::Block
                    | hx_proto::preset::Kind::Looper
                    | hx_proto::preset::Kind::Unknown(_)
            )
        }) {
            let Some(model) = block.model else { continue };
            let (dsp, _) = self.preset.slot_address(slot).unwrap_or((usize::MAX, 0));
            let place = layout.position_of(dsp, slot);
            let info = cat.model_number(model);
            let mut name = info.map_or_else(|| format!("Model {model}"), |m| m.name.clone());
            let mut category = info
                .and_then(|m| cat.category_of(&m.id))
                .and_then(|id| cat.category(id))
                .map_or("Stock", |c| c.name.as_str());
            if model == 380 {
                category = "NAM";
                let number = block.values.first().copied().unwrap_or(0.) as i64;
                name = self
                    .irs
                    .iter()
                    .find(|(i, _)| *i + 1 == number)
                    .map_or_else(|| format!("NAM · slot {number}"), |(_, n)| n.clone());
            }
            let writable = self.editable(slot, cat);
            let params: Vec<_> = block.values.iter().enumerate().map(|(index,&value)| {
                if model == 380 {
                    let names = ["Model","Input","Bass","Mid","Treble","Presence","Ch Vol","Polarity"];
                    let display = match index {1..=6=>format!("{:.1}",value*10.),7 if value==0.=>"Normal".to_owned(),7 if value==1.=>"Inverted".to_owned(),_=>value.to_string()};
                    return json!({"index":index,"name":names.get(index),"value":value,"display":display,
                        "min":0,"max":1,"kind":if index==7 {"Enum"}else{"Continuous"},
                        "choices":if index==7 {Some(["Normal","Inverted"])}else{None},
                        "edit":writable && nam_parameter_wire(&self.preset,slot,index,value).is_some()});
                }
                if let Some(p) = cat.param(model,index) {
                    let edit = writable && p.accepts(value) && p.max>p.min && p.kind!=Kind::Text;
                    let choices = if p.kind==Kind::Enum && p.max-p.min<=128. {
                        Some((p.min as i32..=p.max as i32).map(|i|cat.format(p,i as f32)).collect::<Vec<_>>())
                    } else { None };
                    json!({"index":index,"name":p.name,"value":value,"display":cat.format(p,value),"min":p.min,"max":p.max,
                        "kind":format!("{:?}",p.kind),"edit":edit,"choices":choices})
                } else {json!({"index":index,"name":format!("Parameter {}",index+1),"value":value,"display":value.to_string()})}
            }).collect();
            let cab_params:Vec<_>=block.paired_values.iter().enumerate().map(|(i,&value)|{
                if let Some(p)=block.paired.and_then(|cab|cat.param(cab,i)){
                    json!({"index":i,"name":p.name,"value":value,"display":cat.format(p,value)})
                }else{json!({"index":i,"name":format!("Parameter {}",i+1),"value":value,"display":value.to_string()})}
            }).collect();
            blocks.push(json!({"slot":slot,"model":model,"name":name,"category":category,"enabled":block.enabled,"editable":writable,
                "pairModel":block.paired,"cabParams":cab_params,"pair":block.paired.map(|m|cat.model_number(m).map_or_else(||m.to_string(),|p|p.name.clone())),
                "dsp":dsp,"place":place,"params":params,"manage":self.manageable(slot,cat),
                "canMove":block_move::available(self,slot),"canCopy":block_copy::available(self,cat,slot),
                "namTarget":self.top_cell(slot) && nam_selector(&self.preset,slot).is_some()}));
        }
        json!({"bank":self.identity.0,"index":self.identity.1,"label":profile.slot_label(self.identity.1),"name":self.identity.2,
            "snapshots":snapshots::view(&self.preset),"snapshotColors":cat.menu(hx_catalog::FOOTSWITCH_LED),
            "controllers":parameter_controllers::view(&self.preset,profile.controls),
            "namReady":nam_ready,"captureDirectory":self.irs.iter().map(|(slot,name)|(slot+1,name)).collect::<Vec<_>>(),
            "tempo":self.preset.tempo(),"blocks":blocks,"insertSlots":(1..=8).filter(|s|self.empty_cell(*s)).collect::<Vec<_>>(),"paths":floor_routing::paths(&self.preset,&layout,cat)})
    }
}

#[derive(PartialEq, Clone, Copy)]
struct Target(usize, Option<usize>);

fn apply_event(board: &mut Board, event: i64, args: &Value) -> Option<Target> {
    let field = |k| args.get(k)?.as_i64();
    let data = args.get(106)?;
    let slot = usize::try_from(data.get(98)?.as_i64()?).ok()?;
    if field(82) != Some(0) || board.preset.slot_address(slot) != Some((0, slot)) {
        return None;
    }
    let block = board.preset.slots.get_mut(slot)?;
    block.model?;
    match (event, field(68)?, field(121)?) {
        (49, 5, 17) => {
            block.enabled = data.get(59)?.as_bool()?;
            Some(Target(slot, None))
        }
        (30, 6, 20) if data.get(26)?.as_i64()? == 0 && block.paired.is_none() => {
            let index = usize::try_from(data.get(28)?.as_i64()?).ok()?;
            let v = data.get(119)?;
            *block.values.get_mut(index)? = v
                .as_f32()
                .or_else(|| v.as_bool().map(|b| if b { 1. } else { 0. }))?;
            Some(Target(slot, Some(index)))
        }
        _ => None,
    }
}

fn edit(session: &mut Session, board: &Board, cat: &Catalog, cmd: &Json) -> Result<Target> {
    let slot = cmd["slot"].as_u64().ok_or("Missing slot")? as usize;
    if !board.editable(slot, cat) {
        return Err("Block is read-only".into());
    }
    let block = &board.preset.slots[slot];
    let model = block.model.ok_or("Empty block")?;
    if cmd["model"].as_u64() != Some(model.into()) {
        return Err("Block changed".into());
    }
    if cmd["op"] == "bypass" {
        session.set_enabled(
            slot as i64,
            cmd["value"].as_bool().ok_or("Expected enabled state")?,
        )?;
        return Ok(Target(slot, None));
    }
    if cmd["op"] != "param" {
        return Err("Unknown edit".into());
    }
    let i = cmd["index"].as_u64().ok_or("Missing parameter")? as usize;
    let current = *block.values.get(i).ok_or("Missing value")?;
    let v = cmd["value"].as_f64().ok_or("Missing value")? as f32;
    if model==380 {
        let wire = nam_parameter_wire(&board.preset,slot,i,v).ok_or("Unsupported NAM control value")?;
        session.set_param(slot as i64,i as i64,wire)?;
        return Ok(Target(slot,Some(i)));
    }
    let p = cat.param(model, i).ok_or("Unknown parameter")?;
    if !p.accepts(current)
        || !p.accepts(v)
        || p.max <= p.min
        || p.kind == Kind::Text
        || (p.kind != Kind::Continuous && v.fract() != 0.)
        || (p.kind == Kind::Switch && v != 0. && v != 1.)
    {
        return Err("Value outside supported control range".into());
    }
    session.set_param(slot as i64, i as i64, p.kind.wire(v))?;
    Ok(Target(slot, Some(i)))
}

fn optional_model(value: &Json) -> Result<Option<u32>> {
    if value.is_null() {
        return Ok(None);
    }
    let id = value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("Invalid model number")?;
    Ok(Some(id))
}

fn change_block(
    session: &mut Session,
    board: &mut Board,
    cat: &Catalog,
    cmd: &Json,
) -> Result<String> {
    let slot = cmd["slot"]
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or("Missing cell")?;
    let old_identity = board.identity.clone();
    let original = board.preset.slots.get(slot).cloned();
    // The hardware panel can edit between a click and its USB command.
    *board = Board::read(session)?;
    if board.identity != old_identity || original.as_ref() != board.preset.slots.get(slot) {
        return Ok("Floor changed · select the position again".into());
    }
    let operation = cmd["op"].as_str().unwrap_or("");
    let expected = if operation == "add" {
        if !board.empty_cell(slot) {
            return Ok("That position is occupied or unavailable".into());
        }
        None
    } else {
        let old = cmd["model"].as_u64().and_then(|n| u32::try_from(n).ok());
        let old_pair = optional_model(&cmd["pairModel"])?;
        if !board.manageable(slot, cat)
            || old != board.preset.slots[slot].model
            || old_pair != board.preset.slots[slot].paired
        {
            return Ok("Block changed · select it again".into());
        }
        old.map(|model| (model, old_pair))
    };
    let desired = if operation == "remove" {
        None
    } else {
        let Some(next) = optional_model(&cmd["next"])? else {
            return Ok("Choose a stock model".into());
        };
        let pair = optional_model(&cmd["nextPair"])?;
        if stock_choice(cat, next).is_none()
            || (pair.is_some() && pair != placement_pair(cat, next))
            || Some((next, pair)) == expected
        {
            return Ok("Choose another stock model".into());
        }
        Some((next, pair))
    };
    let sent = match desired {
        Some((model, Some(cab))) => session.set_model_pair(slot as i64, model, cab),
        Some((model, None)) => session.set_model(slot as i64, model),
        None => session.clear_block(slot as i64),
    };
    // Never resend a placement. Read the actual edit buffer even after an error.
    for attempt in 0..3 {
        *board = Board::read(session)?;
        if old_identity != board.identity {
            return Ok("Preset changed · showing the current Floor path".into());
        }
        let matched = match desired {
            Some((model, pair)) => board
                .preset
                .slots
                .get(slot)
                .is_some_and(|b| b.model == Some(model) && b.paired == pair),
            None => board.empty_cell(slot),
        };
        if matched {
            return Ok(format!(
                "Floor confirms · position {slot} {}",
                match operation {
                    "add" => "added",
                    "replace" => "replaced",
                    _ => "cleared",
                }
            ));
        }
        if sent.is_err() || attempt == 2 {
            break;
        }
        thread::sleep(Duration::from_millis(120));
    }
    Ok(match sent {
        Err(e) => format!("Floor declined the change · {e}"),
        Ok(()) => "Change unconfirmed · showing the Floor’s current path".into(),
    })
}

fn read_directory(
    session: &mut Session,
    board: &mut Board,
    cmd: &Json,
    generation: u64,
) -> Result<Json> {
    let mut bank = board.identity.0;
    let mut setlists = Vec::new();
    let rows = if cmd["op"] == "presets" {
        if !cmd["bank"].is_null() {
            bank = cmd["bank"]
                .as_i64()
                .filter(|b| (0..8).contains(b))
                .ok_or("Unknown setlist")?;
        }
        setlists = session.setlists()?;
        if bank as usize >= setlists.len() {
            return Err("Unknown setlist".into());
        }
        session.presets(bank)?.into_iter().enumerate().map(|(index,name)|
            json!({"index":index,"label":session.profile.slot_label(index as i64),"name":name})).collect::<Vec<_>>()
    } else {
        board.irs = session.irs()?;
        board
            .irs
            .iter()
            .filter(|(_, name)| !name.is_empty())
            .map(|(i, name)| json!({"index":i+1,"name":name}))
            .collect()
    };
    Ok(
        json!({"directory":{"kind":cmd["op"],"bank":bank,"rows":rows,"setlists":setlists,"generation":generation}}),
    )
}

// The stock symbol occupying 380 has seven parameters. Our Floor port supplies
// eight. Preserve the actual stored selector's wire type when changing Model.
fn nam_selector(preset: &Preset, slot: usize) -> Option<&Value> {
    let block = preset.slots.get(slot)?;
    if preset.firmware().as_deref() != Some("3.80")
        || block.model != Some(380)
        || block.kind != hx_proto::preset::Kind::Block
        || block.paired.is_some()
        || block.values.len() != 8
        || !(1.0..=128.0).contains(&block.values[0])
        || block.values[0].fract() != 0.0
        || !block.values[1..7].iter().all(|v| (0.0..=1.0).contains(v))
        || !matches!(block.values[7], 0.0 | 1.0)
    {
        return None;
    }
    // Preset::raw_slot already unwraps the slot body (field 20).
    let Value::Array(params) = preset.raw_slot(slot)?.get(11)?.get(4)? else {
        return None;
    };
    let value = params.first()?;
    (matches!(value, Value::F32(_) | Value::F64(_)) || value.as_i64().is_some()).then_some(value)
}

fn nam_wire_value(selector: &Value, number: i64) -> Option<Value> {
    match selector {
        Value::F32(_) => Some(Value::F32(number as f32)),
        Value::F64(_) => Some(Value::F64(number as f64)),
        v if v.as_i64().is_some() => Some(Value::Int(number)),
        _ => None,
    }
}

// Model selection goes through content verification in select_nam. The gain
// and EQ controls preserve the wire type of this specific NAM block.
fn nam_parameter_wire(preset:&Preset,slot:usize,index:usize,value:f32)->Option<Value>{
    nam_selector(preset,slot)?;
    let Value::Array(params)=preset.raw_slot(slot)?.get(11)?.get(4)? else{return None;};
    nam_control_value(params.get(index)?,index,value)
}

fn nam_control_value(current:&Value,index:usize,value:f32)->Option<Value>{
    if !(1..=7).contains(&index) || !(0.0..=1.0).contains(&value) || (index==7 && value.fract()!=0.0) {return None;}
    match current {
        Value::F32(_)=>Some(Value::F32(value)),
        Value::F64(_)=>Some(Value::F64(f64::from(value))),
        Value::Bool(_) if index==7=>Some(Value::Bool(value==1.0)),
        v if index==7 && v.as_i64().is_some()=>Some(Value::Int(value as i64)),
        _=>None
    }
}

#[cfg(test)]
mod nam_tests {
    use super::*;
    #[test]
    fn nam_controls_preserve_wire_types_and_reserve_the_model_selector() {
        assert_eq!(nam_control_value(&Value::F32(0.5),1,0.75),Some(Value::F32(0.75)));
        assert_eq!(nam_control_value(&Value::F64(0.5),6,0.75),Some(Value::F64(0.75)));
        assert_eq!(nam_control_value(&Value::Bool(false),7,1.0),Some(Value::Bool(true)));
        assert_eq!(nam_control_value(&Value::Int(1),7,0.0),Some(Value::Int(0)));
        assert_eq!(nam_control_value(&Value::Int(9),0,0.5),None);
        assert_eq!(nam_control_value(&Value::Bool(true),7,0.5),None);
        assert_eq!(nam_control_value(&Value::Int(0),2,0.5),None);
        assert_eq!(nam_control_value(&Value::F32(0.5),1,f32::NAN),None);
    }
    #[test]
    fn verified_duplicate_falls_back_on_sample_mismatch() {
        let labels = vec![(20, "capture".to_owned()), (11, "capture".to_owned())];
        let expected = "00".repeat(8192);
        let mut visited = Vec::new();
        let result = verified_copy(labels.clone(), &expected, |slot| {
            visited.push(slot);
            let mut samples = vec![0.0; 2048];
            if slot == 19 {
                samples[2047] = 1.0;
            }
            Ok(Some(("capture".to_owned(), samples)))
        })
        .unwrap();
        assert_eq!(visited, vec![19, 10]);
        assert_eq!(result, Some((11, "capture".to_owned())));
        assert_eq!(
            verified_copy(labels, &expected, |_| Ok(Some((
                "wrong name".to_owned(),
                vec![0.0; 2048]
            ))))
            .unwrap(),
            None
        );
    }
    #[test]
    fn wire_type_follows_the_floor_selector() {
        assert_eq!(nam_wire_value(&Value::Int(9), 10), Some(Value::Int(10)));
        assert_eq!(nam_wire_value(&Value::F32(9.0), 10), Some(Value::F32(10.0)));
        assert_eq!(nam_wire_value(&Value::Bool(true), 10), None);
    }

    #[test]
    fn sample_readback_compares_every_encoded_float() {
        let mut samples = vec![0.0; 2048];
        let expected = "00".repeat(8192);
        assert!(payload_equal(&samples, &expected));
        samples[2047] = -0.0;
        assert!(!payload_equal(&samples, &expected));
        assert!(!payload_equal(&samples[..2047], &expected));
    }
}

fn payload_equal(actual: &[f32], expected_hex: &str) -> bool {
    if actual.len() != 2048 || expected_hex.len() != 16384 || !expected_hex.is_ascii() {
        return false;
    }
    actual
        .iter()
        .flat_map(|x| x.to_le_bytes())
        .zip(expected_hex.as_bytes().chunks_exact(2))
        .all(|(byte, s)| {
            std::str::from_utf8(s)
                .ok()
                .and_then(|p| u8::from_str_radix(p, 16).ok())
                == Some(byte)
        })
}

fn verified_copy(
    copies: Vec<(i64, String)>,
    expected_bytes: &str,
    mut read: impl FnMut(i64) -> Result<Option<(String, Vec<f32>)>>,
) -> Result<Option<(i64, String)>> {
    if copies.len() > 128 || copies.iter().any(|(n, _)| !(1..=128).contains(n)) {
        return Err("Invalid capture directory".into());
    }
    for (number, expected_label) in copies {
        if let Some((label, samples)) = read(number - 1)? {
            if label == expected_label && payload_equal(&samples, expected_bytes) {
                return Ok(Some((number, label)));
            }
        }
    }
    Ok(None)
}

fn select_nam(
    session: &mut Session,
    board: &mut Board,
    cmd: &Json,
    attested: bool,
) -> Result<String> {
    let Some(slot) = cmd["slot"]
        .as_u64()
        .and_then(|s| usize::try_from(s).ok())
        .filter(|s| (1..=8).contains(s))
    else {
        return Ok("Choose an empty position or a NAM block on path 1A".into());
    };
    let source = board.preset.slots.get(slot).cloned();
    let identity = board.identity.clone();
    let copies = serde_json::from_value(cmd["captureCopies"].clone())?;
    let expected_bytes = cmd["sampleBytes"]
        .as_str()
        .ok_or("Missing capture verification data")?;
    if !attested {
        return Ok("Recall a NAM A2 Lite preset first".into());
    }
    let Some((number, expected_label)) =
        verified_copy(copies, expected_bytes, |slot| Ok(session.read_ir(slot)?))?
    else {
        return Ok("No installed copy matched the library samples · selection skipped".into());
    };
    *board = Board::read(session)?;
    if board.identity != identity
        || !board.top_cell(slot)
        || board.preset.slots.get(slot) != source.as_ref()
    {
        return Ok("The target changed · select the block again".into());
    }
    if !board
        .irs
        .iter()
        .any(|(i, n)| *i == number - 1 && *n == expected_label)
    {
        return Ok("Capture directory changed · select the profile again".into());
    }
    let adding = board.empty_cell(slot) && cmd["model"].is_null();
    if !adding
        && (cmd["model"].as_u64() != Some(380) || nam_selector(&board.preset, slot).is_none())
    {
        return Ok("Choose an empty position or a NAM A2 Lite block".into());
    }
    if adding {
        let sent = session.set_model(slot as i64, 380);
        *board = Board::read(session)?;
        if board.identity != identity {
            return Ok("Preset changed during placement · showing Floor path".into());
        }
        if nam_selector(&board.preset, slot).is_none() {
            return Ok(match sent {
                Err(e) => format!("Floor declined NAM placement · {e}"),
                Ok(()) => "NAM placement unconfirmed · showing Floor path".into(),
            });
        }
    }
    let block = &board.preset.slots[slot];
    if block.values[0] == number as f32 {
        return Ok("Floor confirms capture".into());
    }
    let created = adding.then(|| block.clone());
    let value = nam_selector(&board.preset, slot)
        .and_then(|v| nam_wire_value(v, number))
        .ok_or("Unsupported NAM selector")?;
    let sent = session.set_param(slot as i64, 0, value);
    *board = Board::read(session)?;
    if board.identity != identity {
        return Ok("Preset changed during selection · showing Floor path".into());
    }
    if nam_selector(&board.preset, slot).is_some()
        && board.preset.slots[slot].values[0] == number as f32
    {
        Ok("Floor confirms capture".into())
    } else {
        let reason = sent
            .err()
            .map_or_else(|| "readback differs".to_owned(), |e| e.to_string());
        // If just our new, untouched default block remains, restore the empty cell.
        // Never clear an existing block or one edited since creation.
        if created.is_some()
            && board.preset.slots.get(slot) == created.as_ref()
            && session.preset_info()? == identity
        {
            let _clear = session.clear_block(slot as i64);
            *board = Board::read(session)?;
            if board.identity == identity && board.empty_cell(slot) {
                return Ok(format!(
                    "Could not apply capture · {reason} · restored empty position"
                ));
            }
        }
        Ok(format!(
            "Capture selection unconfirmed · {reason} · showing the Floor’s actual block"
        ))
    }
}

fn main() -> Result<()> {
    if std::env::args().nth(1).as_deref()==Some("--inspect-backup") {return backup_view::run();}
    let cat = catalog()?;
    if std::env::args().nth(1).as_deref()==Some("--inspect-preset") {
        let file=std::env::args_os().nth(2).ok_or("Choose an original Floor native preset file")?;
        let mut bytes=Vec::new();
        if file=="-" {
            io::stdin().take(1024*1024+1).read_to_end(&mut bytes)?;
        }else{
            std::fs::File::open(file)?.take(1024*1024+1).read_to_end(&mut bytes)?;
        }
        if bytes.len()>1024*1024{return Err("Preset exceeds 1 MiB".into());}
        let preset=Preset::parse(&bytes).ok_or("Could not decode a native preset document")?;
        let profile=*hx_proto::PROFILES.iter().find(|p|p.product_id==0x4248).ok_or("Floor profile missing")?;
        if preset.layout().paths.len()!=2 || preset.firmware().as_deref()!=Some("3.80"){
            return Err("Choose a dual-DSP Floor 3.80 document".into());
        }
        let board=Board{identity:(0,0,String::new()),preset,irs:Vec::new()};
        let ready=board.preset.blocks().any(|(slot,_)|nam_selector(&board.preset,slot).is_some());
        let mut report=board.view(profile,&cat,ready);
        for unavailable in ["bank","index","label","name","captureDirectory"]{
            report.as_object_mut().unwrap().remove(unavailable);
        }
        println!("{}",serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    if std::env::args().any(|a| a == "--placement-catalog") {
        println!("{}", json!(stock_picker(&cat)));
        return Ok(());
    }
    if std::env::args().any(|a| a == "--catalog") {
        for id in [388, 80, 101] {
            let m = cat.model_number(id).ok_or("Missing model")?;
            println!("{} · {} · {} parameters", id, m.name, m.params.len());
        }
        println!("{} catalog models", cat.len());
        return Ok(());
    }
    let devices = hx_usb::list()?;
    let [dev] = devices.as_slice() else {
        return Err("Connect one Helix Floor and close HX Edit".into());
    };
    if dev.profile.product_id != 0x4248 {
        return Err("Helix Floor required".into());
    }
    let mut session = dev.open()?;
    let mut board = Board::read(&mut session)?;
    let mut rev = 1u64;
    let mut generation = 1u64;
    let nam_ready = std::cell::Cell::new(false);
    let emit = |board: &Board, session: &Session, rev, generation, pending: bool, msg: &str| {
        nam_ready.set(
            nam_ready.get()
                || board
                    .preset
                    .blocks()
                    .any(|(slot, _)| nam_selector(&board.preset, slot).is_some()),
        );
        println!(
            "{}",
            json!({"board":board.view(session.profile,&cat,nam_ready.get()),"rev":rev,"generation":generation,"pending":pending,"message":msg})
        );
    };
    emit(&board, &session, rev, generation, false, "Connected");
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in io::stdin()
            .lock()
            .lines()
            .map_while(std::result::Result::ok)
        {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut keep = Instant::now();
    let mut checked = keep;
    let mut inspection_number = 0_u64;
    let mut saves = preset_save::SaveWorkflow::default();
    let mut pending: Option<(Target, Instant)> = None;
    // Startup's read may have left events queued, just like any later read.
    let mut event_boundary = event_sync::ReadBoundary::default();
    loop {
        let cmd = match rx.recv_timeout(Duration::from_millis(45)) {
            Ok(s) if s == "stop" => break,
            Ok(s) => serde_json::from_str::<Json>(&s).ok(),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            _ => None,
        };
        let mut changed = false;
        let mut refresh = false;
        let notes = session.poll_notifications().inspect_err(|_| {
            println!("{}",json!({"usbPhase":"receiving Floor events"}));
        })?;
        let batch = event_boundary.apply(&mut board, &mut pending,
            session.preset_read_epoch(), notes);
        changed |= batch.changed;
        refresh |= batch.refresh;
        if checked.elapsed() >= Duration::from_secs(2) || cmd.is_some() {
            refresh |= session.preset_info().inspect_err(|_| {
                println!("{}",json!({"usbPhase":"checking preset identity"}));
            })? != board.identity;
            checked = Instant::now();
        }
        refresh |= pending.is_some_and(|(_, t)| t.elapsed() >= Duration::from_secs(1));
        if refresh {
            board = Board::read(&mut session).inspect_err(|_| {
                println!("{}",json!({"usbPhase":"refreshing Floor state"}));
            })?;
            generation += 1;
            pending = None;
            changed = true
        }
        if changed {
            rev += 1
        }
        let mut message = "Received from Floor".to_owned();
        if let Some(c) = cmd {
            let installing = c["op"] == "nam-install" || c["op"] == "ir-install";
            let mut installed = None;
            if c["rev"].as_u64() != Some(rev) || pending.is_some() {
                message = "Floor changed · edit skipped".into();
                if matches!(c["op"].as_str(), Some("preset-targets" | "preset-save-preview" | "preset-save-commit")){
                    println!("{}",json!({"presetSave":{"request":c["request"],"error":message}}));
                }
            } else {
                if c["op"] == "presets" || c["op"] == "irs" {
                    match read_directory(&mut session, &mut board, &c, generation) {
                        Ok(directory) => {
                            println!("{directory}");
                            message = "Read from Floor".into();
                        }
                        Err(error) => {
                            message = format!("Directory unavailable · {error}");
                        }
                    }
                } else if c["op"]=="preset-export" {
                    let before=session.preset_info()?;
                    let doc=session.read_preset()?;
                    if before==board.identity && session.preset_info()?==before {
                        let bytes=doc.encode();
                        let hex=bytes.iter().map(|v|format!("{v:02x}")).collect::<String>();
                        println!("{}",json!({"presetFile":{"request":c["request"],"name":before.2,"hex":hex}}));
                        message="Read current edit buffer for export".into();
                    }else{message="Preset changed. Export skipped.".into();}
                } else if matches!(c["op"].as_str(),Some("configured-capture"|"configured-apply")){
                    match configured_blocks::execute(&mut session,&mut board,&cat,&c){
                        Ok(event)=>{println!("{event}");message=event["message"].as_str().unwrap_or("Floor read").into();}
                        Err(e)=>message=e.to_string()
                    }
                    if c["op"]=="configured-apply"{board=Board::read(&mut session)?;rev+=1;generation+=1;}
                } else if matches!(c["op"].as_str(), Some("preset-targets" | "preset-save-preview" | "preset-save-commit")) {
                    match saves.command(&mut session,&board.identity,&c) {
                        Ok(mut event)=>{
                            if event["presetSave"]["phase"]=="saved"{event["directory"]=Json::Null;}
                            println!("{event}");message=event["message"].as_str().unwrap_or("Floor read").to_owned();
                        }
                        Err(e)=>{message=e.to_string();println!("{}",json!({"presetSave":{"request":c["request"],"error":message}}));}
                    }
                    if c["op"]=="preset-save-commit"{board=Board::read(&mut session)?;rev+=1;generation+=1;}
                } else if matches!(c["op"].as_str(), Some("snapshot-select" | "snapshot-metadata" | "snapshot-details" | "snapshot-copy-bypass")) {
                    message = if matches!(c["op"].as_str(),Some("snapshot-select"|"snapshot-metadata")) {
                        let outcome=if c["op"]=="snapshot-metadata"{snapshots::metadata(&mut session,&mut board,&c)}
                            else{snapshots::recall(&mut session,&mut board,&c)};
                        match outcome {
                            Ok(status) => status,
                            Err(e) if e.is::<hx_usb::Error>() => return Err(e),
                            Err(e) => e.to_string(),
                        }
                    } else {
                        "Change snapshot names, colors and bypass copies on the Floor.".into()
                    };
                    generation += 1;
                    rev += 1;
                } else if c["op"]=="route-endpoint" {
                    message=match floor_routing::edit(&mut session,&mut board,&cat,&c){
                        Ok(status)=>status,
                        Err(error)=>{board=Board::read(&mut session)?;format!("Routing edit unconfirmed · {error}")}
                    };
                    generation+=1;
                    rev+=1;
                } else if matches!(c["op"].as_str(),Some("move-block"|"copy-block")) {
                    let result=if c["op"]=="move-block"{block_move::execute(&mut session,&mut board,&c)}
                        else{block_copy::execute(&mut session,&mut board,&cat,&c)};
                    message = match result {
                        Ok(status)=>status,
                        Err(e)=>{board=Board::read(&mut session)?;format!("Block edit unconfirmed · {e}")}
                    };
                    generation += 1;
                    rev += 1;
                } else if matches!(c["op"].as_str(),Some("controller-param"|"controller-bypass")) {
                    message = match parameter_controllers::edit(&mut session,&mut board,&cat,&c) {
                        Ok(status)=>status,
                        Err(e)=>{board=Board::read(&mut session)?;format!("Assignment unconfirmed · {e}")}
                    };
                    generation += 1;
                    rev += 1;
                } else if c["op"] == "inspect-slot" {
                    let number = c["index"].as_i64().filter(|n| (1..=128).contains(n));
                    if let Some(number) = number {
                        match session.read_ir(number-1) {
                            Ok(Some((name, data))) if Some(name.as_str()) == c["name"].as_str() => {
                                let family = if data.starts_with(&[0.5,-0.5,3.0]) {
                                    if data.len()==2048 && data[3]>0.0 && data[4]==1871.0/1024.0 &&
                                        data.iter().all(|v| v.is_finite()) && data[1951..].iter().all(|v| *v==0.0) {
                                        "NAM"
                                    } else { "Unidentified" }
                                } else if !data.is_empty() && data.iter().all(|v| v.is_finite()) { "IR" }
                                else { "Unidentified" };
                                inspection_number += 1;
                                println!("{}", json!({"inspectedSlot":{"index":number,"name":name,"family":family,"generation":generation,"observation":inspection_number}}));
                                message = format!("Slot {number} · {family}");
                            }
                            _ => message = "Could not identify this slot. Refresh the storage list.".into(),
                        }
                    }
                } else if installing {
                    if (nam_ready.get() || c["op"] == "ir-install") && board.preset.firmware().as_deref() == Some("3.80") {
                        match nam_storage::install(&mut session, &c) {
                            Ok(slot) => {
                                installed = Some(slot);
                                message = format!("Slot {slot} installed and verified on Floor");
                            }
                            Err(error) => {
                                message = format!("Installation unconfirmed · {error}");
                            }
                        }
                        board = Board::read(&mut session)?;
                        rev += 1;
                        generation += 1;
                    } else {
                        message = "Recall an A2 Lite preset to identify the engine".into();
                    }
                } else if c["op"] == "nam" {
                    message = match select_nam(&mut session, &mut board, &c, nam_ready.get()) {
                        Ok(status) => status,
                        Err(e) => {
                            board = Board::read(&mut session)?;
                            format!("Could not confirm capture · {e}")
                        }
                    };
                    generation += 1;
                    rev += 1;
                } else if matches!(c["op"].as_str(), Some("add" | "replace" | "remove")) {
                    message = change_block(&mut session, &mut board, &cat, &c)?;
                    generation += 1;
                    rev += 1;
                } else if c["op"] == "recall" {
                    let bank = c["bank"].as_i64().filter(|b| (0..8).contains(b));
                    let index = c["index"].as_u64().filter(|i| *i < 128);
                    let names = match bank {
                        Some(b) => session.presets(b)?,
                        None => Vec::new(),
                    };
                    if let Some(i) = index.filter(|i| {
                        names.get(*i as usize).map(String::as_str) == c["name"].as_str()
                            && *i < names.len() as u64
                    }) {
                        if session.preset_info()? == board.identity {
                            session.select_preset(bank.ok_or("Missing setlist")?, i as i64)?;
                        }
                        board = Board::read(&mut session)?;
                        generation += 1;
                        rev += 1;
                        message = if board.identity.1 == i as i64 && Some(board.identity.0) == bank
                        {
                            "Preset recalled"
                        } else {
                            "Floor changed · check preset"
                        }
                        .into();
                    } else {
                        message = "Preset list changed · recall skipped".into()
                    }
                } else {
                    match edit(&mut session, &board, &cat, &c) {
                        Ok(t) => {
                            pending = Some((t, Instant::now()));
                            message = "Waiting for Floor".into()
                        }
                        Err(e) => {
                            board = Board::read(&mut session)?;
                            generation += 1;
                            rev += 1;
                            message = format!("Edit failed · {e}")
                        }
                    }
                }
            }
            if installing {
                let mut receipt = json!({"installFinished":c["capture"]});
                if let Some(slot) = installed {
                    receipt["installedSlot"] = json!(slot);
                }
                println!("{receipt}");
            }
            changed = true;
        }
        if changed {
            emit(
                &board,
                &session,
                rev,
                generation,
                pending.is_some(),
                &message,
            )
        }
        if keep.elapsed() >= Duration::from_millis(700) {
            let began = Instant::now();
            session.keepalive().inspect_err(|_| {
                println!("{}",json!({"usbPhase":"session keepalive",
                    "elapsedMs":began.elapsed().as_millis(),
                    "channels":session.channel_stats()}));
            })?;
            keep = Instant::now()
        }
    }
    Ok(())
}
