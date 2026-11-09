//! Original Floor 3.80 native move. Frame 6487 in the 07C capture carries
//! opcode 43, with keys 75 and 76 for 1A source and destination positions.
use super::{Board, Result};
use hx_proto::{
    frame::ChannelId,
    msgpack::Value,
    preset::{Kind, Preset},
};
use hx_usb::Session;
use serde_json::Value as Json;
use std::{
    thread,
    time::{Duration, Instant},
};

const MOVE: i64 = 43;

fn args(from: usize, to: usize) -> Value {
    hx_proto::msgmap! {75 => Value::Int(from as i64), 76 => Value::Int(to as i64)}
}

pub(super) fn position(cmd: &Json, field: &str) -> Result<usize> {
    cmd[field]
        .as_u64()
        .filter(|n| (1..=8).contains(n))
        .map(|n| n as usize)
        .ok_or_else(|| "Choose a processing position on path 1A".into())
}

pub(super) fn matches_move(wanted: &Preset, observed: &Preset) -> bool {
    // The device owns the preset's selected editor block. Compare the audio
    // slots and the sections that refer to them, excluding editor selection.
    wanted.slots == observed.slots
        && (0..wanted.slots.len()).all(|i| wanted.raw_slot(i) == observed.raw_slot(i))
        && [3, 4, 10]
            .into_iter()
            .all(|key| wanted.tone.get(key) == observed.tone.get(key))
}

pub(super) fn available(board: &Board, slot: usize) -> bool {
    board.top_cell(slot)
        && board
            .preset
            .slots
            .get(slot)
            .is_some_and(|b| b.kind == Kind::Block && b.model.is_some())
        && board
            .preset
            .layout()
            .paths
            .first()
            .is_some_and(|p| p.lanes.iter().all(|l| l.branch == 0))
}

pub(super) fn execute(session: &mut Session, board: &mut Board, cmd: &Json) -> Result<String> {
    let from = position(cmd, "slot")?;
    let to = position(cmd, "to")?;
    if !available(board, from)
        || !board.empty_cell(to)
        || board.preset.slots[from].model.map(u64::from) != cmd["model"].as_u64()
    {
        return Err("Choose an existing block and an empty position on straight path 1A".into());
    }
    if session.preset_info()? != board.identity {
        return Err("Preset changed before move".into());
    }
    let fresh = session.read_preset()?;
    let current = fresh.slots == board.preset.slots;
    board.preset = fresh;
    if !current || !available(board, from) || !board.empty_cell(to) {
        return Err("The path changed. Choose the block and its destination again.".into());
    }
    // The trace's adjacent empty destination makes a slide and an exchange
    // indistinguishable. Both preserve signal order when dropping into a gap.
    // Confirm either ordering only if all the other state moved with it.
    let bytes = board.preset.encode();
    let mut slide = Preset::parse(&bytes).ok_or("Cannot verify this preset layout")?;
    let mut swap = Preset::parse(&bytes).ok_or("Cannot verify this preset layout")?;
    if !slide.move_slot(from, to) || !swap.swap_slots(from, to) {
        return Err("Cannot verify this move".into());
    }
    // hx-proto moves the controller and eight snapshot tables. The original
    // Floor has an additional snapshot-policy array indexed by slot.
    for (expected, sliding) in [(&mut slide, true), (&mut swap, false)] {
        if let Some(flags) = expected.tone.at_mut(&[10, 13]) {
            let Value::Array(flags) = flags else {
                return Err("Unknown snapshot policy table".into());
            };
            if flags.len() != expected.slots.len() {
                return Err("Snapshot policy layout changed".into());
            }
            if !sliding {
                flags.swap(from, to);
            } else if from < to {
                flags[from..=to].rotate_left(1);
            } else {
                flags[to..=from].rotate_right(1);
            }
        }
    }
    if session.preset_info()? != board.identity {
        return Err("Preset changed before move".into());
    }
    let (status, _) = session.request_full(ChannelId::DATA, MOVE, args(from, to))?;
    if status != 0 {
        return Err(format!("Floor declined the move ({status})").into());
    }

    let until = Instant::now() + Duration::from_millis(900);
    loop {
        let readback = session.read_preset()?;
        if session.preset_info()? != board.identity {
            return Err("Preset changed during move".into());
        }
        let confirmed = matches_move(&slide, &readback) || matches_move(&swap, &readback);
        board.preset = readback;
        if confirmed {
            return Ok(format!(
                "Moved block from 1A {from} to {to} · Floor confirmed"
            ));
        }
        if Instant::now() >= until {
            return Ok("Move unconfirmed · showing the Floor's current path".into());
        }
        thread::sleep(Duration::from_millis(45));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_matches_captured_hx_edit_move() {
        use hx_proto::msgpack;
        // Frame 6487, complete application message after the transport header.
        let frame = [
            0x83, 0x66, 0xcd, 0x03, 0xfc, 0x64, 0x2b, 0x65, 0x82, 0x4b, 1, 0x4c, 2,
        ];
        let request = msgpack::Decoder::new(&frame).value().unwrap();
        assert_eq!(request.get(100).and_then(Value::as_i64), Some(MOVE));
        for k in [75, 76] {
            assert_eq!(
                args(1, 2).get(k).and_then(Value::as_i64),
                request.get(101).unwrap().get(k).and_then(Value::as_i64)
            );
        }
        assert!(position(&serde_json::json!({"to":9}), "to").is_err());
    }
}
