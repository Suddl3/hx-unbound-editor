"""Local TONE3000 capture library for compatible A2 Lite firmware."""
import hashlib
import json
import math
from pathlib import Path
import sqlite3

from inspect_tone_rigs import GEAR_TYPES, capture_labels, inventory, source_category
from prepare_floor_lite_capture import encode_capture
from tone_preset import active_captures

TYPES = tuple(dict.fromkeys(GEAR_TYPES.values()))
_KEEP_INSTRUMENT = object()
KNOWN_SLOTS = {}  # Device placement must be confirmed, never assumed.
# Canonical layer configuration shared by the four previously verified captures.
LITE_LAYERS = "8852da726e9a0ca67d443474b8902664748f982410e7e88a31cc5e93544a493e"


def floor_payload(raw, model):
    variants = [model]
    if model["architecture"] == "SlimmableContainer":
        variants = [s["model"] for s in model["config"]["submodels"]]
    for variant in variants:
        if variant["architecture"] != "WaveNet":
            continue
        config = variant["config"]
        layers = json.dumps(config.get("layers"), sort_keys=True, separators=(",", ":")).encode()
        if hashlib.sha256(layers).hexdigest() != LITE_LAYERS or config.get("head") is not None:
            continue
        if variant.get("sample_rate", model.get("sample_rate")) != 48000:
            return None, "Requires a 48 kHz capture"
        weights = variant["weights"]
        if len(weights) != 1871 or not all(isinstance(w, (int, float)) and math.isfinite(w) for w in weights):
            return None, "Invalid Lite weights"
        try:
            return encode_capture(raw, variant), "A2 Lite"
        except (ValueError, OverflowError, AssertionError) as exc:
            return None, f"Conversion failed: {exc}"
    return None, "No verified A2 Lite layout"


class Library:
    def __init__(self, file, readonly=False):
        self.db = sqlite3.connect(file, uri=readonly)
        self.db.row_factory = sqlite3.Row
        if readonly:
            return
        self.db.executescript("""
          CREATE TABLE IF NOT EXISTS captures(
            id TEXT PRIMARY KEY, name TEXT NOT NULL, original BLOB NOT NULL, wav BLOB,
            compatibility TEXT NOT NULL, file_gear TEXT, override TEXT,
            slot INTEGER UNIQUE CHECK(slot BETWEEN 1 AND 128));
          CREATE TABLE IF NOT EXISTS sources(
            id TEXT NOT NULL, source TEXT NOT NULL, tone_gear TEXT NOT NULL,
            PRIMARY KEY(id, source, tone_gear));
        """)

    def import_nam(self, filename, raw):
        data = json.loads(raw)
        if not isinstance(data, dict) or not isinstance(data.get("architecture"), str):
            raise ValueError("File has no NAM architecture.")
        meta = data.get("metadata")
        name = meta.get("name") if isinstance(meta, dict) else None
        name = name.strip() if isinstance(name, str) else ""
        return self._store_entries([(name or Path(filename).stem, raw, "", filename)]), []

    def import_file(self, path):
        path = Path(path)
        missing = []
        if path.suffix.lower() == ".nam":
            return self.import_nam(path.name, path.read_bytes())
        if path.suffix.lower() == ".t3kpreset":
            rig = inventory(path)
            captures = active_captures(path)
            entries = []
            for chain in rig["paths"].values():
                for block in chain:
                    if block["type"] != "nam":
                        continue
                    name, raw = captures[block["activeModelId"]]
                    if raw is None:
                        missing.append(name)
                    else:
                        entries.append((name, raw, block["gear"] or "", rig["preset"]))
        else:
            raise ValueError("Choose a .nam profile or .t3kpreset file.")

        return self._store_entries(entries), missing

    def _store_entries(self, entries):
        ids = set()
        with self.db:
            self.db.execute("BEGIN IMMEDIATE")
            for name, raw, gear, source in entries:
                key = hashlib.sha256(raw).hexdigest()
                if not self.db.execute("SELECT 1 FROM captures WHERE id=?", (key,)).fetchone():
                    data = json.loads(raw)
                    if not isinstance(data, dict) or not isinstance(data.get("architecture"), str):
                        raise ValueError("File has no NAM architecture.")
                    wav, status = floor_payload(raw, data)
                    label = capture_labels(gear, raw)
                    slot = KNOWN_SLOTS.get(key) if wav else None
                    if slot and self.db.execute("SELECT 1 FROM captures WHERE slot=?", (slot,)).fetchone():
                        slot = None
                    self.db.execute("""INSERT INTO captures(id,name,original,wav,compatibility,file_gear,slot)
                                       VALUES(?,?,?,?,?,?,?)""",
                                    (key, name, raw, wav, status, label["nam_gear"], slot))
                self.db.execute("INSERT OR IGNORE INTO sources VALUES(?,?,?)", (key, source, gear))
                ids.add(key)
        return ids

    def rows(self):
        rows = []
        for r in self.db.execute("""SELECT id,name,compatibility,file_gear,override,slot,
                                    wav IS NOT NULL AS ready FROM captures
                                    ORDER BY slot IS NULL,slot,name COLLATE NOCASE,id"""):
            row = dict(r)
            sources = self.db.execute("SELECT source,tone_gear FROM sources WHERE id=? ORDER BY source", (r["id"],))
            pairs = list(sources)
            row["sources"] = ", ".join(sorted({s for s, _ in pairs}))
            gears = {g for _, g in pairs if g}
            row["listing"] = ", ".join(sorted(gears))
            row["source_type"] = source_category((*gears, r["file_gear"]))
            row["type"] = r["override"] or row["source_type"] or "Needs review"
            rows.append(row)
        return rows

    def classify(self, key, category, *, instrument=_KEEP_INSTRUMENT):
        if category not in (*TYPES, None):
            raise ValueError("Unknown capture type")
        if instrument is not _KEEP_INSTRUMENT and instrument not in ("Guitar", "Bass", None):
            raise ValueError("Unknown capture instrument")
        with self.db:
            changed = self.db.execute("UPDATE captures SET override=? WHERE id=?", (category, key))
            if not changed.rowcount and instrument is not _KEEP_INSTRUMENT:
                raise ValueError("Capture not found in the library")
            if instrument is not _KEEP_INSTRUMENT:
                self.db.execute("""CREATE TABLE IF NOT EXISTS capture_instruments(
                    id TEXT PRIMARY KEY REFERENCES captures(id) ON DELETE CASCADE,
                    instrument TEXT NOT NULL CHECK(instrument IN ('Guitar','Bass')))""")
                if instrument is None:
                    self.db.execute("DELETE FROM capture_instruments WHERE id=?", (key,))
                else:
                    self.db.execute("""INSERT INTO capture_instruments(id,instrument) VALUES(?,?)
                        ON CONFLICT(id) DO UPDATE SET instrument=excluded.instrument""", (key,instrument))

    def map_slot(self, key, slot):
        if slot is not None and not 1 <= slot <= 128:
            raise ValueError("Use a slot from 1 to 128.")
        with self.db:
            other = self.db.execute("SELECT name FROM captures WHERE slot=? AND id<>?", (slot, key)).fetchone()
            if other:
                raise ValueError(f"Slot {slot} is mapped to {other[0]}. Clear that mapping first.")
            if not self.db.execute("UPDATE captures SET slot=? WHERE id=? AND wav IS NOT NULL",
                                   (slot, key)).rowcount:
                raise ValueError("This profile has no compatible WAV.")

    def wav(self, key):
        row = self.db.execute("SELECT wav FROM captures WHERE id=?", (key,)).fetchone()
        if not row or not row[0]:
            raise ValueError("This profile has no compatible WAV.")
        return row[0]
