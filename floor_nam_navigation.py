"""Resolve names for the live NAM shelf from local captures and the Floor directory."""
import json
import hashlib
import math
from pathlib import Path
import re
import struct
import sqlite3
from contextlib import closing
from functools import lru_cache

from capture_library import Library, LITE_LAYERS


LEGACY_LABELS = {
    "D-Lux-556392-Lite": "eed1c147db2eea77b517dabe1f45d836d7e42200d200643bec48dc9f6ab456fa",
    "Plexi-815147-Lite": "372c59c62bce8e9796a4389956cfbca0fdffd0e574e5e690c11983f1e0ce65a0",
}
# Two exact capture identities with short names used throughout the listening tests.
REFERENCE_NAMES = {LEGACY_LABELS["D-Lux-556392-Lite"]: "D-Lux",
                   LEGACY_LABELS["Plexi-815147-Lite"]: "Plexi"}

# SHA-256 of the canonical JSON layer table in the simulated 12,146-weight
# D-Lux Full submodel. Learned weights and head_scale differ between captures.
FULL_LAYERS = "1e6b80e8e9de9ea5c59ae823419ca44e393fe78a4dca23733a4eac245c566203"
LAYOUTS = {LITE_LAYERS: ("A2 Lite",1871), FULL_LAYERS: ("A2 Full",12146)}


def source_engines(model):
    if not isinstance(model,dict):
        return ()
    candidates = [model]
    if model.get("architecture") == "SlimmableContainer":
        config = model.get("config")
        submodels = config.get("submodels") if isinstance(config,dict) else None
        candidates = [s.get("model") for s in submodels if isinstance(s,dict)] if isinstance(submodels,list) else []
    found = set()
    for variant in candidates:
        if not isinstance(variant,dict) or variant.get("architecture") != "WaveNet":
            continue
        config = variant.get("config")
        if (not isinstance(config,dict) or config.get("head") is not None or
                variant.get("sample_rate",model.get("sample_rate")) != 48000):
            continue
        layers = json.dumps(config.get("layers"),sort_keys=True,separators=(",",":")).encode()
        layout = LAYOUTS.get(hashlib.sha256(layers).hexdigest())
        weights = variant.get("weights")
        if not layout or not isinstance(weights,list) or len(weights) != layout[1]:
            continue
        # These are binary32 kernels. Reject non-finite and unrepresentable weights.
        max_float = float.fromhex("0x1.fffffep127")
        if all(type(w) in (int,float) and -max_float <= w <= max_float for w in weights):
            found.add(layout[0])
    return tuple(e for e in ("A2 Lite","A2 Full") if e in found)


@lru_cache(maxsize=2048)
def _library_engines(uri, key):
    with closing(sqlite3.connect(uri,uri=True)) as db:
        row = db.execute("SELECT original FROM captures WHERE id=?", (key,)).fetchone()
    if not row or hashlib.sha256(row[0]).hexdigest() != key:
        return ()
    return source_engines(json.loads(row[0]))


def resolve_label(label, capture_ids):
    if label.upper().startswith("IR "):
        return None
    suffix = re.search(r"-([0-9a-f]{8})(?:-Lite)?$", label, re.I)
    if suffix:
        matches = [key for key in capture_ids if key.startswith(suffix[1].lower())]
        return matches[0] if len(matches) == 1 else None
    return LEGACY_LABELS.get(label)


def read_slot_labels(directory, capture_ids):
    slots = {}
    for line in directory.splitlines():
        if not line.strip():
            continue
        row = re.fullmatch(r"[ \t]*(\d+)[ \t]{2,}(.+)", line)
        if not row:
            raise ValueError("Could not read the Floor IR directory.")
        slot, name = int(row[1]), row[2].strip()
        if not 1 <= slot <= 128 or slot in slots or not name:
            raise ValueError("Invalid row in the Floor IR directory.")
        slots[slot] = {"name": name, "ref": resolve_label(name, capture_ids)}
    return slots


def installed_matches(directory, capture_ids):
    matches = {}
    for slot, name in directory:
        if not isinstance(slot, int) or not 1 <= slot <= 128 or not isinstance(name, str):
            continue
        key = resolve_label(name, capture_ids)
        if key is not None:
            matches.setdefault(key, []).append((slot, name))
    return matches


def instrument_label(metadata):
    explicit = {str(metadata[field]).strip().casefold() for field in ("instrument", "instrument_type")
                if metadata.get(field)}
    labels = {"guitar": "Guitar", "electric guitar": "Guitar", "acoustic guitar": "Guitar",
              "bass": "Bass", "bass guitar": "Bass", "electric bass": "Bass"}
    values = {labels.get(value, "Unknown") for value in explicit}
    return values.pop() if len(values) == 1 else "Unknown"


def nam_rows(library_path, directory):
    if not library_path.exists():
        return []
    uri = Path(library_path).resolve().as_uri() + "?mode=ro"
    lib = Library(uri, readonly=True)
    try:
        lib.db.execute("BEGIN")
        # JSON extraction avoids transferring the original model weights for a list view.
        metadata = {}
        for key, meta in lib.db.execute("""
            SELECT id, json_extract(CAST(original AS TEXT),'$.metadata') FROM captures
            WHERE json_type(CAST(original AS TEXT),'$.metadata')='object'"""):
            metadata[key] = json.loads(meta)
        captures = lib.rows()
        has_instruments = lib.db.execute(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='capture_instruments'").fetchone()
        instruments = dict(lib.db.execute("SELECT id,instrument FROM capture_instruments")) if has_instruments else {}
        matches = installed_matches(directory, [c["id"] for c in captures])
        rows = []
        for c in captures:
            key = c["id"]
            kind = c["type"] if c["type"] != "Needs review" else "Unknown"
            rows.append(dict(id=key, name=REFERENCE_NAMES.get(key,c["name"]), sourceName=c["name"],
                             fileGear=c["file_gear"], listingGear=c["listing"], sourceKind=c["source_type"],
                             kind=kind, instrument=instruments.get(key) or instrument_label(metadata.get(key,{})),
                             overrideKind=c["override"], overrideInstrument=instruments.get(key),
                             engines=list(_library_engines(uri,key)),
                             compatibility=c["compatibility"], engine="A2 Lite" if c["ready"] else None,
                             ready=bool(c["ready"]), installed=bool(c["ready"] and matches.get(key)),
                             floorCopies=matches.get(key, [])))
        return rows
    finally:
        lib.db.close()


def classify_capture(library_path, capture, *, kind, instrument):
    if not isinstance(capture,str) or not re.fullmatch(r"[0-9a-f]{64}",capture):
        raise ValueError("Invalid capture identity")
    if not Path(library_path).is_file():
        raise ValueError("The local capture library is missing")
    lib = Library(library_path)
    try:
        lib.classify(capture, kind, instrument=instrument)
    finally:
        lib.db.close()
    _named_installs.cache_clear()


def capture_payload(library_path, key):
    if not isinstance(key,str) or not re.fullmatch(r"[0-9a-f]{64}",key):
        raise ValueError("Invalid capture identity")
    lib = Library(Path(library_path).resolve().as_uri() + "?mode=ro", readonly=True)
    try:
        wav = lib.wav(key)
    finally:
        lib.db.close()
    if (len(wav) != 8236 or wav[:4] != b"RIFF" or wav[8:16] != b"WAVEfmt " or
            struct.unpack_from("<I",wav,4)[0] != 8228 or struct.unpack_from("<I",wav,16)[0] != 16 or
            struct.unpack_from("<HHIIHH", wav,20) != (3,1,48000,192000,4,32) or wav[36:40] != b"data"
            or struct.unpack_from("<I",wav,40)[0] != 8192):
        raise ValueError("The converted A2 Lite payload is invalid")
    values = struct.unpack_from("<2048f", wav,44)
    if (values[:3] != (.5,-.5,3.) or values[3] <= 0 or values[4]*1024 != 1871 or
            not all(map(math.isfinite,values)) or any(values[8+1943:])):
        raise ValueError("The converted capture does not match the verified A2 Lite layout")
    return wav[44:]


def prepare_selection(library_path, directory, key, preferred=None):
    payload = capture_payload(library_path,key)
    lib = Library(Path(library_path).resolve().as_uri() + "?mode=ro", readonly=True)
    try:
        ids = [row[0] for row in lib.db.execute("SELECT id FROM captures")]
    finally:
        lib.db.close()
    matches = installed_matches(directory, ids).get(key)
    if not matches:
        raise ValueError("This capture needs a verified compatible copy on the Floor.")
    matches.sort(key=lambda copy: (copy[0] != preferred, copy[0]))
    return {"captureCopies":matches, "sampleBytes":payload.hex()}


@lru_cache(maxsize=4)
def _named_installs(library_path, directory, stamps):
    rows = nam_rows(library_path, directory)
    matches = installed_matches(directory, [row["id"] for row in rows])
    return {slot: row for row in rows if row["ready"] for slot, _ in matches.get(row["id"], ())}


def name_live_blocks(library_path, board):
    blocks = [b for b in board["blocks"] if b["model"] == 380]
    if not blocks or not library_path.is_file():
        return
    wal = Path(str(library_path) + "-wal")
    try:
        wal_stamp = wal.stat().st_mtime_ns
    except FileNotFoundError:
        wal_stamp = None
    directory = tuple(tuple(pair) for pair in board["captureDirectory"])
    names = _named_installs(library_path, directory, (library_path.stat().st_mtime_ns, wal_stamp))
    for block in blocks:
        params = block.get("params") or []
        row = names.get(params[0]["value"]) if params else None
        if row:
            block.update(name=row["name"], namId=row["id"], captureKind=row["kind"], instrument=row["instrument"])
