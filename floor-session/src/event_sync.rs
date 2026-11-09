//! Preserve direct knob updates except across an active-buffer read boundary.
//! A notification carries a slot, not a preset identity or device revision.
use super::*;

#[derive(Default)]
pub(super) struct ReadBoundary {
    observed: Option<u64>,
}

#[derive(Default, Debug)]
pub(super) struct Batch {
    pub changed: bool,
    pub refresh: bool,
}

impl ReadBoundary {
    pub fn apply(
        &mut self,
        board: &mut Board,
        pending: &mut Option<(Target, Instant)>,
        read_epoch: u64,
        notes: Vec<(i64, Value)>,
    ) -> Batch {
        let crossed_read = self.observed != Some(read_epoch);
        self.observed = Some(read_epoch);
        let mut batch = Batch::default();
        for (event, args) in notes {
            if event == 22
                && args.get(68).and_then(Value::as_i64) == Some(10)
                && args.get(121).and_then(Value::as_i64) == Some(27)
            {
                continue;
            }
            // These deltas may predate the full read or refer to the old
            // occupant of a moved slot. Re-read once for the entire batch;
            // never publish them over the newer board or count them as echoes.
            if crossed_read {
                // Snapshot metadata has a typed snapshot address. If the field
                // already equals the authoritative readback, ignoring its echo
                // cannot overwrite newer state and avoids a redundant full read.
                if snapshots::event_matches_readback(&board.preset, event, &args) {
                    continue;
                }
                batch.refresh = true;
                continue;
            }
            if snapshots::apply_event(&mut board.preset, event, &args) {
                batch.changed = true;
            } else if let Some(target) = apply_event(board, event, &args) {
                if pending.is_some_and(|(wanted, _)| wanted == target) {
                    *pending = None;
                }
                batch.changed = true;
            } else {
                batch.refresh = true;
            }
        }
        batch
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const RAW: &[u8] = include_bytes!("../../vendor/tonepush/crates/hx-proto/tests/preset.bin");

    fn board() -> (Board, usize) {
        let preset = Preset::parse(RAW).unwrap();
        let slot = preset.blocks().find(|(s, b)| {
            preset.slot_address(*s) == Some((0, *s))
                && b.paired.is_none() && !b.values.is_empty()
        }).unwrap().0;
        (Board { identity: (0, 26, "New board".into()), preset, irs: vec![] }, slot)
    }

    fn knob(slot: usize, value: f32) -> (i64, Value) {
        (30, hx_proto::msgmap! {
            82 => Value::Int(0), 68 => Value::Int(6), 121 => Value::Int(20),
            106 => hx_proto::msgmap! {
                98 => Value::Int(slot as i64), 26 => Value::Int(0),
                28 => Value::Int(0), 119 => Value::F32(value),
            },
        })
    }

    #[test]
    fn buffered_old_knobs_cannot_overwrite_readback_or_confirm_a_pending_edit() {
        let (mut board, slot) = board();
        board.preset.slots[slot].values[0] = 0.9;
        let mut guard = ReadBoundary { observed: Some(1) };
        let mut pending = Some((Target(slot, Some(0)), Instant::now()));
        let batch = guard.apply(&mut board, &mut pending, 2,
            vec![knob(slot, 0.2), knob(slot, 0.3)]);
        assert!(batch.refresh);
        assert!(!batch.changed);
        assert_eq!(board.preset.slots[slot].values[0], 0.9);
        assert!(pending.is_some());
    }

    #[test]
    fn new_preset_in_same_slot_is_not_patched_by_the_previous_presets_event() {
        let (mut board, slot) = board();
        let mut guard = ReadBoundary { observed: Some(3) };
        let expected = board.preset.slots[slot].values.clone();
        let batch = guard.apply(&mut board, &mut None, 4, vec![knob(slot, -100.0)]);
        assert!(batch.refresh);
        assert_eq!(board.identity.1, 26);
        assert_eq!(board.preset.slots[slot].values, expected);
    }

    #[test]
    fn ordinary_knob_burst_stays_direct_and_a_quiet_boundary_does_not_reread() {
        let (mut board, slot) = board();
        let mut guard = ReadBoundary::default();
        let quiet = guard.apply(&mut board, &mut None, 1, vec![]);
        assert!(!quiet.refresh && !quiet.changed);
        let mut pending = Some((Target(slot, Some(0)), Instant::now()));
        let batch = guard.apply(&mut board, &mut pending, 1,
            vec![knob(slot, 0.2), knob(slot, 0.7)]);
        assert!(batch.changed && !batch.refresh);
        assert!(pending.is_none());
        assert_eq!(board.preset.slots[slot].values[0], 0.7);
    }

    fn snapshot_name(index: i64, name: &str) -> (i64, Value) {
        (43, hx_proto::msgmap! {92 => Value::Int(index), 109 => Value::Str(name.into())})
    }

    fn snapshot_color(index: i64, color: i64) -> (i64, Value) {
        (48, hx_proto::msgmap! {92 => Value::Int(index), 66 => Value::Int(color)})
    }

    #[test]
    fn verified_snapshot_echoes_do_not_request_another_full_read() {
        let (mut board, _) = board();
        assert!(board.preset.set_snapshot_name(0, "USB One"));
        assert!(board.preset.set_snapshot_colour(0, 8));
        let mut guard = ReadBoundary { observed: Some(1) };
        let batch = guard.apply(&mut board, &mut None, 3,
            vec![snapshot_name(0, "USB One"), snapshot_color(0, 8)]);
        assert!(!batch.refresh, "confirmed metadata echoes must not force readback again");
        assert!(!batch.changed, "identical metadata must not invalidate the next command revision");
    }

    #[test]
    fn conflicting_snapshot_echoes_cannot_overwrite_newer_readback() {
        let (mut board, slot) = board();
        assert!(board.preset.set_snapshot_name(0, "USB One"));
        assert!(board.preset.set_snapshot_colour(0, 8));
        let mut guard = ReadBoundary { observed: Some(1) };
        let mut pending = Some((Target(slot, Some(0)), Instant::now()));
        let batch = guard.apply(&mut board, &mut pending, 3,
            vec![snapshot_name(0, "Old Name"), snapshot_color(0, 6)]);
        assert!(batch.refresh && !batch.changed);
        assert!(pending.is_some());
        let snapshots = board.preset.snapshot_details();
        assert_eq!(snapshots[0].name, "USB One");
        assert_eq!(snapshots[0].colour, Some(8));
    }

    #[test]
    fn matching_metadata_does_not_hide_an_ambiguous_knob_or_unknown_event() {
        let (mut board, slot) = board();
        assert!(board.preset.set_snapshot_name(0, "USB One"));
        for other in [knob(slot, 0.1), snapshot_name(8, "USB One"),
                      (999, Value::Nil), (43, Value::Nil)] {
            let mut guard = ReadBoundary { observed: Some(1) };
            let before = board.preset.slots[slot].values.clone();
            let batch = guard.apply(&mut board, &mut None, 3,
                vec![snapshot_name(0, "USB One"), other]);
            assert!(batch.refresh && !batch.changed);
            assert_eq!(board.preset.slots[slot].values, before);
        }
    }

    #[test]
    fn metadata_match_is_field_and_snapshot_specific_and_never_confirms_a_knob() {
        let (mut board, slot) = board();
        assert!(board.preset.set_snapshot_name(0, "USB One"));
        assert!(board.preset.set_snapshot_colour(0, 8));
        assert!(board.preset.set_snapshot_name(1, "Other"));
        assert!(board.preset.set_snapshot_colour(1, 6));
        for note in [snapshot_name(1, "USB One"), snapshot_color(1, 8),
                     snapshot_color(0, -1), snapshot_color(0, 12),
                     (43, hx_proto::msgmap! {92=>Value::Int(0),109=>Value::Int(4)}),
                     (48, hx_proto::msgmap! {92=>Value::Int(0)})] {
            assert!(!snapshots::event_matches_readback(&board.preset, note.0, &note.1));
        }
        let mut pending = Some((Target(slot, Some(0)), Instant::now()));
        let mut guard = ReadBoundary { observed: Some(1) };
        let batch = guard.apply(&mut board, &mut pending, 2,
            vec![snapshot_name(0, "USB One"), snapshot_color(0, 8)]);
        assert!(!batch.changed && !batch.refresh);
        assert!(pending.is_some());
        let quiet = guard.apply(&mut board, &mut pending, 2, vec![]);
        assert!(!quiet.changed && !quiet.refresh);
    }

    #[test]
    #[ignore = "requires local snapshot replay fixtures generated by audit_snapshot_captures.py"]
    fn recorded_snapshot_echoes_match_the_recorded_preset_without_a_refresh() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../build/floor-session/snapshot-replay");
        let raw = std::fs::read(root.join("preset.bin")).unwrap();
        let preset = Preset::parse(&raw).unwrap();
        let details = preset.snapshot_details();
        assert_eq!(details[1].name, "TraceCheck");
        assert_eq!(details[1].colour, Some(8));
        let notes: Vec<_> = ["name-event.bin", "color-event.bin"].into_iter().map(|file| {
            let bytes = std::fs::read(root.join(file)).unwrap();
            let values = hx_proto::msgpack::Decoder::decode_all(&bytes).unwrap();
            assert_eq!(values.len(), 1);
            (values[0].get(105).and_then(Value::as_i64).unwrap(),
             values[0].get(106).unwrap().clone())
        }).collect();
        let mut board = Board { identity: (3, 26, "New Preset".into()), preset, irs: vec![] };
        let mut guard = ReadBoundary { observed: Some(1) };
        let batch = guard.apply(&mut board, &mut None, 3, notes);
        assert!(!batch.changed && !batch.refresh);
    }
}
