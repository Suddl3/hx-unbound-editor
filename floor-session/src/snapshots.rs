use hx_proto::{msgpack::Value, preset::Preset};
use hx_usb::Session;
use serde_json::{json, Value as Json};
use super::Result;
const SNAPSHOT_SECTION:i64=10;
const ACTIVE_SNAPSHOT:i64=6;

pub fn active(preset: &Preset) -> Option<usize> {
    let number = preset.tone.get(SNAPSHOT_SECTION)?.get(ACTIVE_SNAPSHOT)?.as_i64()?;
    let index = usize::try_from(number).ok()?;
    (index < 8 && matches!(preset.tone.get(SNAPSHOT_SECTION)?.get(10)?, Value::Array(entries) if index < entries.len())).then_some(index)
}

pub fn view(preset: &Preset) -> Json {
    let slots: Vec<_> = preset.snapshot_details().iter().enumerate().map(|(index, s)| {
        json!({"index":index,"name":s.name,"color":s.colour,"tempo":s.tempo,"valid":s.valid,
               "enabled":s.enabled})
    }).collect();
    json!({"active":active(preset),"slots":slots})
}

fn index(cmd: &Json, field: &str, count: usize) -> Result<usize> {
    cmd[field].as_u64().and_then(|i| usize::try_from(i).ok())
        .filter(|i| *i < count && *i < 8).ok_or_else(|| "Snapshot position changed".into())
}

fn metadata_args(slot: usize, name: Option<&str>, color: Option<i64>) -> Result<(i64, Value)> {
    if slot >= 8 { return Err("Unknown snapshot".into()); }
    let address=Value::Int(slot as i64);
    match (name,color) {
        (Some(text),None) if !text.trim().is_empty() && text.len()<=16 && text.bytes().all(|b| (32..=126).contains(&b)) =>
            Ok((89,hx_proto::msgmap!{92=>address,109=>Value::Str(text.to_owned())})),
        (None,Some(n @ 0..=11)) => Ok((98,hx_proto::msgmap!{92=>address,66=>Value::Int(n)})),
        _ => Err("Use a snapshot name of 1–16 printable ASCII characters and a listed LED color".into()),
    }
}

pub fn apply_event(preset: &mut Preset, event: i64, args: &Value) -> bool {
    let Some(slot)=args.get(92).and_then(Value::as_i64).filter(|i|(0..8).contains(i)) else {return false;};
    match event {
        43 => args.get(109).and_then(Value::as_str).is_some_and(|name|preset.set_snapshot_name(slot as usize,name)),
        48 => args.get(66).and_then(Value::as_i64).is_some_and(|n|preset.set_snapshot_colour(slot as usize,n)),
        _ => false
    }
}

/// An identical metadata echo carries no information beyond the full readback.
/// This must never apply a conflicting event or confirm a pending block edit.
pub fn event_matches_readback(preset: &Preset, event: i64, args: &Value) -> bool {
    if !matches!(event, 43 | 48) { return false; }
    let Some(index) = args.get(92).and_then(Value::as_i64).filter(|i| (0..8).contains(i)) else {
        return false;
    };
    let details = preset.snapshot_details();
    let Some(snapshot) = details.get(index as usize) else { return false; };
    match event {
        43 => args.get(109).and_then(Value::as_str).is_some_and(|name| name == snapshot.name),
        48 => args.get(66).and_then(Value::as_i64)
            .filter(|color| (0..=11).contains(color))
            .is_some_and(|color| snapshot.colour == Some(color)),
        _ => false,
    }
}

pub fn metadata(session: &mut Session, board: &mut super::Board, cmd: &Json) -> Result<String> {
    use hx_proto::frame::ChannelId;
    let slot=index(cmd,"from",8)?;
    let name=cmd["name"].as_str().ok_or("Missing snapshot name")?;
    let color=cmd["color"].as_i64().ok_or("Missing LED color")?;
    // Validate both fields before sending either one.
    let rename=metadata_args(slot,Some(name),None)?;
    let recolor=metadata_args(slot,None,Some(color))?;
    let fresh=session.read_preset()?;
    if session.preset_info()?!=board.identity {return Err("Preset changed. Reopen snapshot settings.".into());}
    board.preset=fresh;
    let snapshots=board.preset.snapshot_details();
    let before=snapshots.get(slot).ok_or("Snapshot missing")?;
    if board.preset.firmware().as_deref()!=Some("3.80") || active(&board.preset)!=Some(slot)
        || Some(before.name.as_str())!=cmd["wasName"].as_str() || before.colour.is_none() || before.colour!=cmd["wasColor"].as_i64() {
        return Err("Snapshot changed. Reopen snapshot settings.".into());
    }
    let mut refused=false;
    for (changed,(op,args)) in [(name!=before.name,rename),(Some(color)!=before.colour,recolor)] {
        if changed {
            println!("{}",json!({"usbPhase":if op==89 {"rename snapshot"} else {"set snapshot color"}}));
            if session.request_full(ChannelId::DATA,op,args)?.0!=0 {refused=true;break;}
        }
    }
    println!("{}",json!({"usbPhase":"verify snapshot settings"}));
    let back=session.read_preset()?;
    if session.preset_info()?!=board.identity {return Err("Preset changed during snapshot verification".into());}
    board.preset=back;
    let stored=board.preset.snapshot_details();
    if refused || active(&board.preset)!=Some(slot) || !stored.get(slot).is_some_and(|s|s.name==name && s.colour==Some(color)) {
        return Err("Snapshot settings differ on readback. Showing the Floor's values.".into());
    }
    Ok(format!("Snapshot {} settings confirmed",slot+1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hx_proto::msgpack::Encoder;
    #[test]
    fn metadata_arguments_match_floor_recordings() {
        let (op,args)=metadata_args(0,Some("TraceOne"),None).unwrap();
        assert_eq!(op,89);
        assert_eq!(Encoder::encode(&args),b"\x82\x5c\x00\x6d\xa9TraceOne\x00");
        let (op,args)=metadata_args(1,None,Some(6)).unwrap();
        assert_eq!(op,98);
        assert_eq!(Encoder::encode(&args),b"\x82\x5c\x01\x42\x06");
        assert!(metadata_args(8,None,Some(6)).is_err());
        assert!(metadata_args(1,Some("bad\0name"),None).is_err());
    }
}

/// Recall uses a short command. Never upload a preset from the snapshot panel.
pub fn recall(session: &mut Session, board: &mut super::Board, cmd: &Json) -> Result<String> {
    let preset = session.read_preset()?;
    let count = preset.snapshots().len();
    let from = index(cmd, "from", count)?;
    let dest = index(cmd, "index", count)?;
    if session.preset_info()? != board.identity {
        return Err("Preset changed before snapshot recall".into());
    }
    board.preset = preset;
    if board.preset.firmware().as_deref() != Some("3.80") || active(&board.preset) != Some(from) {
        return Err("Snapshot changed. Check the current selection.".into());
    }
    if from != dest {
        println!("{}", json!({"usbPhase":"recall snapshot"}));
        session.select_snapshot(dest as i64)?;
        println!("{}", json!({"usbPhase":"verify snapshot recall"}));
        for attempt in 0..4 {
            let actual = session.read_preset()?;
            if session.preset_info()? != board.identity {
                return Err("Preset changed during snapshot recall".into());
            }
            board.preset = actual;
            if active(&board.preset) == Some(dest) { break; }
            if attempt < 3 { std::thread::sleep(std::time::Duration::from_millis(70)); }
        }
    }
    if active(&board.preset) != Some(dest) {
        return Err("Snapshot recall was not confirmed by the Floor".into());
    }
    Ok(format!("Snapshot {} recalled", dest + 1))
}
