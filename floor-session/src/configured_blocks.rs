//! Editor-local stock settings. Assignments and snapshots belong to the preset.
use super::{placement_pair, stock_choice, Board, Result};
use hx_catalog::Catalog;
use hx_proto::{
    msgpack::{Decoder, Encoder, Value},
    preset::Preset,
};
use hx_usb::Session;
use serde_json::{json, Value as Json};

fn decode(text: &str) -> Result<Value> {
    if !(8..=32768).contains(&text.len())
        || text.len() % 2 != 0
        || !text.bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err("Invalid configured block data".into());
    }
    let bytes = text
        .as_bytes()
        .chunks_exact(2)
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut values = Decoder::decode_all(&bytes)?;
    if values.len() != 1 {
        return Err("Invalid configured block data".into());
    }
    Ok(values.remove(0))
}

fn confirm(expected: &Preset, actual: &Preset, slot: usize) -> bool {
    expected.copy_slot(slot) == actual.copy_slot(slot)
        && expected.slots == actual.slots
        && expected.layout() == actual.layout()
        && [3, 4, 10]
            .iter()
            .all(|key| expected.tone.get(*key) == actual.tone.get(*key))
}

fn overlay(p: &Preset, pos: usize, raw: &Value, model: u32) -> Result<Preset> {
    let mut with_settings =
        Preset::parse(&p.encode()).ok_or("Cannot reconstruct current preset")?;
    if !with_settings.paste_slot(pos, raw) || with_settings.slots[pos].model != Some(model) {
        return Err("This configured block is incompatible with the destination".into());
    }
    let bytes = with_settings.encode();
    let mut checked = Preset::parse(&bytes).ok_or("The configured block did not round-trip")?;
    checked.settle_branches();
    if checked.encode() != bytes {
        return Err("Applying these settings would also edit an empty branch".into());
    }
    Ok(checked)
}

pub(super) fn execute(
    s: &mut Session,
    board: &mut Board,
    cat: &Catalog,
    cmd: &Json,
) -> Result<Json> {
    let pos = cmd["slot"]
        .as_u64()
        .filter(|x| (1..=8).contains(x))
        .ok_or("Choose a position on path 1A")? as usize;
    let model = cmd["model"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or("Choose a stock block")?;
    if board.preset.firmware().as_deref() != Some("3.80") || s.preset_info()? != board.identity {
        return Err("Floor changed. Open the current preset again.".into());
    }
    let fresh = s.read_preset()?;
    if fresh.slots != board.preset.slots
        || fresh.layout() != board.preset.layout()
        || s.preset_info()? != board.identity
    {
        return Err("The path changed. Select the current position again.".into());
    }
    board.preset = fresh;
    if cmd["op"] == "configured-capture" {
        if !board.manageable(pos, cat) || board.preset.slots[pos].model != Some(model) {
            return Err("Select a supported stock block on path 1A".into());
        }
        let name = cmd["name"]
            .as_str()
            .map(str::trim)
            .filter(|s| {
                !s.is_empty() && s.chars().count() <= 48 && !s.chars().any(char::is_control)
            })
            .ok_or("Choose a name up to 48 characters")?;
        let data = Encoder::encode(
            &board
                .preset
                .copy_slot(pos)
                .ok_or("Stock block is missing")?,
        );
        if data.len() > 16384 {
            return Err("This stock block is too large for this library".into());
        }
        let hex = data.iter().map(|v| format!("{v:02x}")).collect::<String>();
        return Ok(
            json!({"configuredBlock":{"name":name,"model":model,"hex":hex},"message":"Read configured block from Floor"}),
        );
    }
    if cmd["op"] != "configured-apply" || !board.empty_cell(pos) {
        return Err("Choose an empty supported position on path 1A".into());
    }
    let raw = decode(cmd["blockHex"].as_str().ok_or("Configured block missing")?)?;
    let expected = overlay(&board.preset, pos, &raw, model)?;
    let pair = expected.slots[pos].paired;
    if stock_choice(cat, model).is_none()
        || (pair.is_some() && pair != placement_pair(cat, model))
        || s.preset_info()? != board.identity
        || s.read_preset()?.encode() != board.preset.encode()
    {
        return Err("The block is unsupported or the Floor changed. Placement skipped.".into());
    }
    // Native allocation asks the actual Floor whether this model/pair fits its
    // DSP budget. Only a confirmed allocation can receive the captured settings.
    let source = board.identity.clone();
    let snapshot = board.preset.tone.get(10).and_then(|v| v.get(6)).cloned();
    let admission = match pair {
        Some(cab) => s.set_model_pair(pos as i64, model, cab),
        None => s.set_model(pos as i64, model),
    };
    *board = Board::read(s)?;
    if admission.is_err()
        || board.identity != source
        || !board.manageable(pos, cat)
        || board.preset.slots[pos].model != Some(model)
        || board.preset.slots[pos].paired != pair
        || board.preset.tone.get(10).and_then(|v| v.get(6)) != snapshot.as_ref()
    {
        return Err("The Floor did not confirm allocation on the same snapshot. A default block may be present.".into());
    }
    let staged = board.preset.encode();
    let expected = overlay(&board.preset, pos, &raw, model)
        .map_err(|e| format!("Block placed with its defaults. Settings skipped: {e}"))?;
    if s.preset_info()? != source || s.read_preset()?.encode() != staged {
        return Err("The path changed after allocation. Settings skipped; the new block may retain its defaults.".into());
    }
    s.write_preset(&expected).map_err(|e| {
        format!("Block allocated; custom settings unconfirmed. Check the current path. {e}")
    })?;
    let back = s.read_preset()?;
    if s.preset_info()? != board.identity || !confirm(&expected, &back, pos) {
        return Err("Configured block write sent. The Floor did not confirm the full block and surrounding preset.".into());
    }
    board.preset = back;
    Ok(json!({"message":format!("Applied configured favorite in 1A {pos} · Floor verified")}))
}

#[cfg(test)]
mod tests {
    use super::*;
    const RAW:&[u8]=include_bytes!("../../vendor/tonepush/crates/hx-proto/tests/preset.bin");

    #[test]
    fn clipboard_roundtrip_carries_knobs_and_preserves_neighbours() {
        let source = Preset::parse(RAW).unwrap();
        let (from, b) = source.blocks().find(|(_, b)| !b.values.is_empty()).unwrap();
        let dest = source
            .slots
            .iter()
            .enumerate()
            .find(|(i, s)| {
                *i != from
                    && matches!(
                        s.kind,
                        hx_proto::preset::Kind::Empty | hx_proto::preset::Kind::Block
                    )
            })
            .unwrap()
            .0;
        let original = source.copy_slot(from).unwrap();
        let raw = Encoder::encode(&original)
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>();
        let mut pasted = Preset::parse(RAW).unwrap();
        assert!(pasted.paste_slot(dest, &decode(&raw).unwrap()));
        let back = Preset::parse(&pasted.encode()).unwrap();
        assert!(confirm(&pasted, &back, dest));
        assert_eq!(back.slots[dest].values, b.values);
        assert_eq!(back.copy_slot(dest), Some(original));
        assert_eq!(back.slots[from], source.slots[from]);
        assert!(!confirm(&pasted, &source, dest));
    }

    #[test]
    fn apply_uses_the_allocated_document_and_rejects_another_model() {
        let mut allocated = Preset::parse(RAW).unwrap();
        allocated.settle_branches();
        let (slot, block) = allocated
            .blocks()
            .find(|(_, b)| !b.values.is_empty())
            .unwrap();
        let model = block.model.unwrap();
        let raw = allocated.copy_slot(slot).unwrap();
        assert!(overlay(&allocated, slot, &raw, model.wrapping_add(1)).is_err());
        let configured = overlay(&allocated, slot, &raw, model).unwrap();
        assert!(confirm(&configured, &allocated, slot));
        assert_eq!(configured.tone.get(10), allocated.tone.get(10));
    }
}
