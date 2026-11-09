use hx_proto::{msgpack::Encoder, preset::Preset};
use hx_usb::Session;
use serde_json::{json, Value as Json};
use std::{
    error::Error,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Identity = (i64, i64, String);

trait SaveDevice {
    fn label(&self, slot: i64) -> String;
    fn identity(&mut self) -> Result<Identity>;
    fn banks(&mut self) -> Result<Vec<String>>;
    fn names(&mut self, bank: i64) -> Result<Vec<String>>;
    fn loaded(&mut self) -> Result<Preset>;
    fn stored(&mut self, bank: i64, slot: i64) -> Result<Option<Preset>>;
    fn commit(&mut self, p: &Preview, current: &Preset) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    const FIXTURE:&[u8]=include_bytes!("../../vendor/tonepush/crates/hx-proto/tests/preset.bin");
    struct Floor {
        current: Preset,
        target: Option<Vec<u8>>,
        name: String,
        writes: usize,
        mismatch: bool,
    }
    fn source() -> Identity {
        (0, 0, "Original".into())
    }
    fn floor() -> Floor {
        let mut current = Preset::parse(FIXTURE).unwrap();
        *current.tone.get_mut(7).unwrap().get_mut(35).unwrap() =
            hx_proto::msgpack::Value::UInt(0x03800000);
        Floor {
            current,
            target: None,
            name: "Spare".into(),
            writes: 0,
            mismatch: false,
        }
    }
    impl SaveDevice for Floor {
        fn label(&self, slot: i64) -> String {
            if slot == 0 { "01A" } else { "01B" }.into()
        }
        fn identity(&mut self) -> Result<Identity> {
            Ok(source())
        }
        fn banks(&mut self) -> Result<Vec<String>> {
            Ok(vec!["Personal".into()])
        }
        fn names(&mut self, _: i64) -> Result<Vec<String>> {
            Ok(vec!["Original".into(), self.name.clone()])
        }
        fn loaded(&mut self) -> Result<Preset> {
            Ok(Preset::parse(&self.current.encode()).unwrap())
        }
        fn stored(&mut self, _: i64, _: i64) -> Result<Option<Preset>> {
            Ok(self.target.as_deref().and_then(Preset::parse))
        }
        fn commit(&mut self, p: &Preview, preset: &Preset) -> Result<()> {
            self.writes += 1;
            self.name = p.name.clone();
            let mut d = Preset::parse(&preset.encode()).unwrap();
            if self.mismatch {
                assert!(d.set_tempo(98.25));
            }
            self.target = Some(d.encode());
            Ok(())
        }
    }
    fn review() -> Json {
        json!({"request":"txn","op":"preset-save-preview","mode":"copy","bank":0,"index":1,
        "name":"Favorite","expectedName":"Spare","source":{"bank":0,"label":"01A","name":"Original"}})
    }
    fn commit() -> Json {
        json!({"op":"preset-save-commit","request":"txn"})
    }
    #[test]
    fn verified_once_and_target_guard() {
        let mut w = SaveWorkflow::default();
        let mut s = floor();
        assert!(w.dispatch(&mut s, &source(), &commit()).is_err());
        w.dispatch(&mut s, &source(), &review()).unwrap();
        assert_eq!(s.writes, 0);
        s.target = Some(s.current.encode());
        assert!(w.dispatch(&mut s, &source(), &commit()).is_err());
        assert_eq!(s.writes, 0);
        w.dispatch(&mut s, &source(), &review()).unwrap();
        assert_eq!(
            w.dispatch(&mut s, &source(), &commit()).unwrap()["presetSave"]["phase"],
            "saved"
        );
        assert!(w.dispatch(&mut s, &source(), &commit()).is_err());
        assert_eq!(s.writes, 1);
    }
    #[test]
    fn changed_source_and_failed_verification() {
        let mut w = SaveWorkflow::default();
        let mut s = floor();
        w.dispatch(&mut s, &source(), &review()).unwrap();
        assert!(s.current.set_tempo(77.5));
        assert!(w.dispatch(&mut s, &source(), &commit()).is_err());
        assert_eq!(s.writes, 0);
        w.dispatch(&mut s, &source(), &review()).unwrap();
        s.mismatch = true;
        assert!(w
            .dispatch(&mut s, &source(), &commit())
            .unwrap_err()
            .to_string()
            .contains("readback"));
        assert_eq!(s.writes, 1);
    }
    #[test]
    fn save_current_has_its_own_destination_guard() {
        let mut w = SaveWorkflow::default();
        let mut s = floor();
        let mut c = review();
        c["mode"] = json!("current");
        assert!(w.dispatch(&mut s, &source(), &c).is_err());
        c["index"] = json!(0);
        c["expectedName"] = json!("Original");
        c["name"] = json!("Original");
        w.dispatch(&mut s, &source(), &c).unwrap();
        assert_eq!(
            w.dispatch(&mut s, &source(), &commit()).unwrap()["presetSave"]["copy"],
            false
        );
    }
}

impl SaveDevice for Session {
    fn label(&self, slot: i64) -> String {
        self.profile.slot_label(slot)
    }
    fn identity(&mut self) -> Result<Identity> {
        Ok(self.preset_info()?)
    }
    fn banks(&mut self) -> Result<Vec<String>> {
        Ok(self.setlists()?)
    }
    fn names(&mut self, bank: i64) -> Result<Vec<String>> {
        Ok(self.presets(bank)?)
    }
    fn loaded(&mut self) -> Result<Preset> {
        Ok(self.read_preset()?)
    }
    fn stored(&mut self, bank: i64, slot: i64) -> Result<Option<Preset>> {
        Ok(self.read_preset_at(bank, slot)?)
    }
    fn commit(&mut self, p: &Preview, current: &Preset) -> Result<()> {
        if p.copy {
            self.write_preset_at(p.bank, p.slot, &p.name, current)?
        } else {
            self.save_preset(p.bank, p.slot, &p.name)?
        }
        Ok(())
    }
}

// The preview and commit form one transaction. Keep both source and destination
// documents in the USB helper; a stale browser cannot substitute either one.
struct Preview {
    id: String,
    source: Identity,
    source_bytes: Vec<u8>,
    bank: i64,
    slot: i64,
    previous: String,
    previous_bytes: Option<Vec<u8>>,
    name: String,
    copy: bool,
    at: Instant,
}

#[derive(Default)]
pub struct SaveWorkflow {
    preview: Option<Preview>,
}

fn index(cmd: &Json, field: &str, limit: i64) -> Result<i64> {
    cmd[field]
        .as_i64()
        .filter(|i| (0..limit).contains(i))
        .ok_or_else(|| "Invalid preset address".into())
}

fn save_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > 16 || !name.bytes().all(|b| (32..=126).contains(&b)) {
        return Err("Use 1–16 printable ASCII characters for the preset name.".into());
    }
    Ok(())
}

// FETCH_PRESET omits the runtime build stamp and has a different section table.
// Compare the raw tone sections covering settings, all DSPs, slots, assignments,
// and snapshots. In particular, a matching destination name alone proves nothing.
fn same_tone(a: &Preset, b: &Preset) -> bool {
    fn payload(p: &Preset) -> Vec<u8> {
        let mut tone = p.tone.clone();
        // Helix preset metadata (field 7) includes the running firmware build.
        if let Some(meta) = tone.get_mut(7) {
            *meta = hx_proto::msgpack::Value::Nil;
        }
        Encoder::encode(&tone)
    }
    payload(a) == payload(b)
}

fn directory_name(s: &mut impl SaveDevice, bank: i64, slot: i64) -> Result<String> {
    s.names(bank)?
        .get(slot as usize)
        .cloned()
        .ok_or_else(|| "Preset location missing in this setlist.".into())
}

impl SaveWorkflow {
    pub fn command(&mut self, s: &mut Session, source: &Identity, cmd: &Json) -> Result<Json> {
        self.dispatch(s, source, cmd)
    }
    fn dispatch(&mut self, s: &mut impl SaveDevice, source: &Identity, cmd: &Json) -> Result<Json> {
        let id = cmd["request"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 64)
            .ok_or("Missing save request identity")?;
        let op = cmd["op"].as_str().ok_or("Unknown save operation")?;
        let event = match op {
            "preset-targets" => {
                let bank = index(cmd, "bank", 8)?;
                let sets = s.banks()?;
                if bank as usize >= sets.len() {
                    return Err("Setlist is unavailable".into());
                }
                let rows = s
                    .names(bank)?
                    .into_iter()
                    .enumerate()
                    .map(|(i, name)| json!({"index":i,"label":s.label(i as i64),"name":name}))
                    .collect::<Vec<_>>();
                json!({"request":id,"phase":"targets","bank":bank,"setlists":sets,"rows":rows})
            }
            "preset-save-preview" => {
                self.preview = None;
                let bank = index(cmd, "bank", 8)?;
                let slot = index(cmd, "index", 128)?;
                let src = &cmd["source"];
                if src["bank"].as_i64() != Some(source.0)
                    || src["label"].as_str() != Some(s.label(source.1).as_str())
                    || src["name"].as_str() != Some(source.2.as_str())
                    || s.identity()? != *source
                {
                    return Err("The current preset changed. Open Save again.".into());
                }
                let copy = cmd["mode"] == "copy";
                if !(copy || cmd["mode"] == "current")
                    || copy == ((bank, slot) == (source.0, source.1))
                {
                    return Err("Select a different destination for Save As.".into());
                }
                let name = cmd["name"].as_str().ok_or("Missing preset name")?.trim();
                if copy {
                    save_name(name)?;
                } else if name != source.2 {
                    return Err("Current preset name changed.".into());
                }
                let previous = directory_name(s, bank, slot)?;
                if cmd["expectedName"].as_str() != Some(previous.as_str()) {
                    return Err("Destination name changed. Reopen the preset list.".into());
                }
                let previous_bytes = s.stored(bank, slot)?.map(|p| p.encode());
                let preset = s.loaded()?;
                if preset.firmware().as_deref() != Some("3.80") || s.identity()? != *source {
                    return Err("The Floor preset or firmware changed. Save skipped.".into());
                }
                let target = s.label(slot);
                let event = json!({"request":id,"phase":"preview","bank":bank,"index":slot,"label":target,
                    "previous":previous,"name":name,"copy":copy,"source":source.2});
                self.preview = Some(Preview {
                    id: id.into(),
                    source: source.clone(),
                    source_bytes: preset.encode(),
                    bank,
                    slot,
                    previous,
                    previous_bytes,
                    name: name.into(),
                    copy,
                    at: Instant::now(),
                });
                event
            }
            "preset-save-commit" => {
                let p = self.preview.take().ok_or("Open a fresh save preview.")?;
                if p.id != id
                    || p.at.elapsed() > Duration::from_secs(120)
                    || p.source != *source
                    || s.identity()? != *source
                {
                    return Err(
                        "Save preview expired or the source changed. Open Save again.".into(),
                    );
                }
                let prior = s.stored(p.bank, p.slot)?.map(|d| d.encode());
                if prior != p.previous_bytes || directory_name(s, p.bank, p.slot)? != p.previous {
                    return Err("Destination changed since confirmation. Save skipped.".into());
                }
                let wanted = s.loaded()?;
                if wanted.encode() != p.source_bytes || s.identity()? != p.source {
                    return Err("Edits changed since confirmation. Open Save again.".into());
                }
                // Write once. Save As writes the reviewed edit-buffer document to
                // the chosen slot without recalling it or discarding current edits.
                println!("{}",json!({"usbPhase":if p.copy {"write destination preset"} else {"store current preset"}}));
                s.commit(&p, &wanted).map_err(|e| {
                    format!("Save unconfirmed; check the destination before retrying. {e}")
                })?;
                println!("{}",json!({"usbPhase":"verify stored preset"}));
                let stored = s.stored(p.bank, p.slot)?;
                if directory_name(s, p.bank, p.slot)? != p.name
                    || !stored.is_some_and(|d| same_tone(&wanted, &d))
                {
                    return Err("Save was sent. The stored name or tone differs on readback; check the destination before retrying.".into());
                }
                json!({"request":id,"phase":"saved","bank":p.bank,"index":p.slot,"name":p.name,
                       "label":s.label(p.slot),"copy":p.copy})
            }
            _ => return Err("Unknown save operation".into()),
        };
        let message = if event["phase"] == "saved" {
            format!(
                "Saved and verified {} {}",
                event["label"].as_str().unwrap_or(""),
                event["name"].as_str().unwrap_or("")
            )
        } else {
            "Read from Floor".into()
        };
        Ok(json!({"presetSave":event,"message":message}))
    }
}
