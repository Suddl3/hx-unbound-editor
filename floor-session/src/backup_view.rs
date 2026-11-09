//! Local, read-only projection of the setlists in an HX Edit backup.
use hx_catalog::{read_backup_setlists, BackupPreset, Catalog, Container};
use serde_json::{json, Value};
use std::io::{self, Read};

const FILE_LIMIT: usize = 16 * 1024 * 1024;
const SETLIST_BUDGET: u64 = 48 * 1024 * 1024;

fn summarize(p: &BackupPreset, bank_width: u8, cat: Option<&Catalog>) -> Value {
    let tone = &p.hlx["data"]["tone"];
    let mut snapshots = Vec::new();
    for index in 0..8 {
        let snapshot = &tone[format!("snapshot{index}")];
        if snapshot.is_object() {
            snapshots.push(json!({"index":index, "name":snapshot["@name"].as_str(),
                "valid":snapshot["@valid"].as_bool(), "tempo":snapshot["@tempo"].as_f64()}));
        }
    }

    let mut blocks = Vec::new();
    for dsp in 0..2 {
        let dsp_key = format!("dsp{dsp}");
        let Some(chain) = tone[&dsp_key].as_object() else {
            continue;
        };
        for (key, b) in chain {
            if !key
                .strip_prefix("block")
                .is_some_and(|s| s.parse::<u8>().is_ok())
            {
                continue;
            }
            let symbol = b["@model"].as_str().unwrap_or("");
            let name = match symbol {
                "" | "HelixPlugin_AppDSPFlow1Input" => "Unidentified model",
                id => cat
                    .and_then(|c| c.model(id))
                    .map_or(id, |m| m.name.as_str()),
            };
            let enabled = b["@enabled"].as_bool();
            let states: Vec<_> = (0..8)
                .map(|i| {
                    let s = &tone[format!("snapshot{i}")];
                    if s["@valid"].as_bool() != Some(true) {
                        return None;
                    }
                    if b["@no_snapshot_bypass"].as_bool() == Some(true) {
                        return enabled;
                    }
                    s["blocks"][&dsp_key][key].as_bool()
                })
                .collect();
            blocks.push(json!({"dsp":dsp,"path":b["@path"].as_u64(),
                "position":b["@position"].as_u64(), "key":key,"name":name,
                "symbol":symbol,"enabled":enabled,"states":states}));
        }
    }
    blocks.sort_by(|a, b| {
        let location = |v: &Value| {
            (
                v["dsp"].as_u64(),
                v["path"].as_u64(),
                v["position"].as_u64(),
            )
        };
        location(a)
            .cmp(&location(b))
            .then(a["key"].as_str().cmp(&b["key"].as_str()))
    });
    let label = hx_proto::rpc::slot_label_for(p.index as i64, bank_width);
    json!({"index":p.index,"label":label,"name":p.name,"blocks":blocks,
        "snapshots":snapshots,"currentSnapshot":tone["global"]["@current_snapshot"].as_u64()})
}

fn inspect(bytes: &[u8], catalog: Option<&Catalog>) -> super::Result<Value> {
    if bytes.len() > FILE_LIMIT {
        return Err("Backup exceeds 16 MiB".into());
    }
    if bytes
        .get(16..24)
        .is_some_and(|b| u64::from_le_bytes(b.try_into().unwrap()) > 2048)
    {
        return Err("Unsupported backup block count".into());
    }
    let container = Container::parse(bytes)?;
    // The catalog decoder bounds each inflated stream. Also bound their aggregate
    // before materializing JSON for every setlist in the bundle.
    let mut total = 0;
    for b in &container.blocks {
        if b.tag.starts_with(b"SL") || b.tag.ends_with(b"LS") && b.tag != *b"MNLS" {
            total = u64::checked_add(total, b.raw_len).ok_or("Invalid setlist size")?;
        }
    }
    if total > SETLIST_BUDGET {
        return Err("Combined setlists exceed 48 MiB".into());
    }
    drop(container);
    let mut lists = Vec::new();
    for list in read_backup_setlists(bytes)? {
        let slots = &list.backup.presets;
        if list.index > 7 || slots.len() > 128 {
            return Err("Unsupported setlist dimensions".into());
        }
        // Empty Floor setlists can omit device IDs. Their 128 slots use four letters.
        let presets: Vec<_> = slots
            .iter()
            .map(|p| {
                summarize(
                    p,
                    if slots.len() == 128 {
                        4
                    } else {
                        p.presets_per_bank
                    },
                    catalog,
                )
            })
            .collect();
        lists.push(json!({"index":list.index,"name":list.backup.name,"presets":presets}));
    }
    Ok(json!({"setlists":lists}))
}

pub(super) fn run() -> super::Result<()> {
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(FILE_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)?;
    let view = inspect(&bytes, super::catalog().ok().as_ref())?;
    serde_json::to_writer(io::stdout().lock(), &view)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_named_effect_uses_per_snapshot_bypass() {
        let preset = BackupPreset {
            index: 3,
            presets_per_bank: 4,
            name: "New Preset".into(),
            empty: true,
            hlx: json!({"data":{"tone":{
                "dsp0":{"block1":{"@model":"HD2_Test","@enabled":true,"@position":2,"@path":0},
                    "block0":{"@model":"HelixPlugin_AppDSPFlow1Input","@enabled":false,"@no_snapshot_bypass":true,"@position":0,"@path":0}},
                "snapshot0":{"@valid":true,"@name":"Verse","blocks":{"dsp0":{"block1":false,"block0":true}}},
                "snapshot1":{"@valid":true,"@name":"Solo","blocks":{"dsp0":{"block1":true}}},
                "snapshot2":{"@valid":false,"blocks":{"dsp0":{"block1":true}}}
            }}}),
        };
        let p = summarize(&preset, 4, None);
        assert_eq!(p["label"], "01D");
        assert_eq!(p["blocks"][0]["name"], "Unidentified model");
        assert_eq!(p["blocks"][0]["states"][0], false);
        assert_eq!(p["blocks"][1]["states"][0], false);
        assert_eq!(p["blocks"][1]["states"][1], true);
        assert!(p["blocks"][1]["states"][2].is_null());
    }

    #[test]
    fn rejects_corrupt_and_oversize() {
        assert!(inspect(b"AF6L", None).is_err());
        let bytes = Container {
            version: 1,
            blocks: vec![hx_catalog::Block {
                tag: *b"00LS",
                compressed: true,
                raw_len: SETLIST_BUDGET + 1,
                stored: vec![0],
            }],
        }
        .encode();
        assert!(inspect(&bytes, None)
            .unwrap_err()
            .to_string()
            .contains("48 MiB"));
    }

    #[test]
    #[ignore = "requires the local November 5 Floor backup"]
    fn original_floor_library() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../build/usb-nam/local-backup.hxb");
        let j = inspect(&std::fs::read(path).unwrap(), None).unwrap();
        let lists = j["setlists"].as_array().unwrap();
        assert_eq!(lists.len(), 8);
        assert!(lists
            .iter()
            .all(|s| s["presets"].as_array().unwrap().len() == 128));
        assert_eq!(lists[0]["presets"][127]["label"], "32D");
        assert!(lists[0]["presets"][0]["blocks"].as_array().unwrap().len() > 0);
        assert_eq!(
            lists[0]["presets"][0]["snapshots"]
                .as_array()
                .unwrap()
                .len(),
            8
        );
    }
}
