"""List the ordered blocks and controls in local TONE3000 presets for Floor import planning."""
import argparse
import json
from pathlib import Path
import struct

from tone_preset import integer, read_preset


def setting(raw):
    if raw == b"\x02":
        return True
    if raw == b"\x03":
        return False
    if raw[:1] == b"\x01":
        return integer(raw)
    if raw[:1] == b"\x04" and len(raw) == 9:
        return struct.unpack_from("<d", raw, 1)[0]
    if raw.startswith(b"\x05") and raw.endswith(b"\0"):
        return raw[1:-1].decode()
    raise ValueError("Unsupported setting type")


def controls(fields, excluded=()):
    return {k: setting(v) for k, v in fields.items() if k not in excluded}


GEAR_TYPES = {
    "amp": "Amp head", "amp-cab": "Amp + cab", "full-rig": "Amp + cab",
    "pedal": "Pedal", "pedal-amp": "Pedal + amp", "amp-pedal-cab": "Pedal + amp + cab",
    "preamp": "Preamp", "studio": "Studio", "cab": "Cab", "space": "Space",
    "outboard": "Outboard", "experimental": "Experimental",
}


def source_category(labels):
    """Use a type only when every supplied gear label resolves to it."""
    categories = {GEAR_TYPES.get(label.strip().lower().replace("_", "-"))
                  for label in labels if isinstance(label, str) and label.strip()}
    return categories.pop() if len(categories) == 1 else None


def capture_labels(tone_gear, payload):
    """Preserve the listing and individual NAM labels so conflicts stay visible."""
    file_gear = None
    if payload:
        try:
            model = json.loads(payload)
        except (ValueError, UnicodeDecodeError):
            model = {}
        metadata = model.get("metadata") if isinstance(model, dict) else None
        if isinstance(metadata, dict):
            gear = metadata.get("gear_type")
            file_gear = gear if isinstance(gear, str) else None
    category = source_category((tone_gear, file_gear))
    return {"tone_gear": tone_gear, "nam_gear": file_gear, "category": category,
            "needs_review": category is None}


def inventory(path, *, parsed=None):
    fields, sections = parsed if parsed is not None else read_preset(path)
    rig = {"preset": setting(fields["name"]), "file": path.name, "paths": {}, "master": {}}
    for kind, props, children in sections:
        if kind == "Params":
            rig["master"] = {setting(p["id"]): setting(p["value"])
                             for tag, p, _ in children if tag == "Param"}
        if kind != "ChainSnapshot":
            continue
        rig["routing"] = controls(props)
        for side, _, blocks in children:
            if side not in ("ChainBlocks", "RightChainBlocks"):
                continue
            chain = []
            for pos, (tag, block, groups) in enumerate(blocks, 1):
                if tag != "ChainBlock" or setting(block["type"]) == "insert":
                    continue
                rec = controls(block, ("toneJson",))
                tone = json.loads(setting(block["toneJson"])) if "toneJson" in block else {}
                model = next((m for m in tone.get("models", [])
                              if m.get("id") == rec.get("activeModelId")), {})
                rec.update(position=pos, title=tone.get("title"), gear=tone.get("gear"),
                           model=model.get("name"), embedded=False)
                payload = None
                for group, attr, entries in groups:
                    if group == "Eq":
                        rec["eq"] = {"settings": controls(attr),
                                     "bands": [controls(p) for _, p, _ in entries]}
                    if group == "ModelCache":
                        for tag, p, _ in entries:
                            if tag == "CachedModel" and integer(p["modelId"]) == rec.get("activeModelId"):
                                raw = p.get("data", b"")
                                if raw.startswith(b"\x08") and len(raw) > 1:
                                    rec["embedded"] = True
                                    if rec["type"] == "nam":
                                        payload = raw[1:]
                if rec["type"] == "nam":
                    rec["capture_labels"] = capture_labels(tone.get("gear"), payload)
                chain.append(rec)
            rig["paths"][side] = chain
    return rig


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="One .t3kpreset file or a folder of saved presets")
    src = parser.parse_args().source
    files = sorted(src.glob("*.t3kpreset")) if src.is_dir() else [src]
    if not files:
        parser.error("No .t3kpreset files found")
    print(json.dumps([inventory(p) for p in files], indent=2, allow_nan=False))
