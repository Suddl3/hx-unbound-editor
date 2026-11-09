"""Prepare a mono 48 kHz cabinet or reverb impulse for verified USB upload."""
from contextlib import closing
import hashlib
import math
from pathlib import Path
import sqlite3
import struct

from capture_library import Library
from floor_slot_labels import floor_label


def impulse_samples(raw):
    if raw[:4] != b"RIFF" or raw[8:12] != b"WAVE" or int.from_bytes(raw[4:8], "little") + 8 != len(raw):
        raise ValueError("Choose a complete RIFF WAV file.")
    pos, chunks = 12, {}
    while pos + 8 <= len(raw):
        kind, size = struct.unpack_from("<4sI", raw, pos)
        end = pos + 8 + size
        if end > len(raw) or kind in chunks:
            raise ValueError("Invalid WAV chunk.")
        if kind in (b"fmt ", b"data"):
            chunks[kind] = raw[pos+8:end]
        pos = end + size % 2
    fmt, data = chunks.get(b"fmt ", b""), chunks.get(b"data", b"")
    if len(fmt) < 16 or not data:
        raise ValueError("WAV format or samples missing.")
    tag, channels, rate, _, width, bits = struct.unpack_from("<HHIIHH", fmt)
    if channels != 1 or rate != 48000 or (tag, bits) not in ((1,16),(1,24),(1,32),(3,32)):
        raise ValueError("Use a mono 48 kHz WAV, 16/24/32-bit PCM or 32-bit float.")
    if width != bits // 8 or len(data) % width or len(data) // width > 2048:
        raise ValueError("Use an impulse of up to 2,048 samples. Longer reverb tails do not fit.")
    if tag == 3:
        values = [v for v, in struct.iter_unpack("<f", data)]
    else:
        scale = 2 ** (bits - 1)
        values = [int.from_bytes(data[i:i+width], "little", signed=True) / scale
                  for i in range(0, len(data), width)]
    if values[:3] == [.5, -.5, 3.]:
        raise ValueError("This WAV contains packed model data. Use the NAM library.")
    if not all(math.isfinite(v) for v in values):
        raise ValueError("The impulse contains non-finite samples.")
    return struct.pack("<2048f", *(values + [0.] * (2048 - len(values))))


def prepare_ir_install(path, directory, filename, kind, raw):
    prefixes = {"cab":"IR Cab", "reverb":"IR Rev", "other":"IR"}
    if kind not in prefixes or Path(filename).suffix.lower() != ".wav":
        raise ValueError("Choose the impulse type and a WAV file.")
    samples = impulse_samples(raw)
    digest = hashlib.sha256(samples).hexdigest()
    key = f"ir:{kind}:{digest}"
    label = floor_label(prefixes[kind], Path(filename).stem, digest)
    lib = Library(path)
    with closing(lib.db) as db, db:
        db.execute("BEGIN IMMEDIATE")
        db.execute("CREATE TABLE IF NOT EXISTS pending_ir(id TEXT PRIMARY KEY,slot INTEGER UNIQUE NOT NULL)")
        pending = dict(db.execute("SELECT id,slot FROM pending_ir"))
        reserved = {slot for other, slot in pending.items() if other != key}
        reserved.update(slot for slot, in db.execute("SELECT slot FROM captures WHERE slot IS NOT NULL"))
        names = dict(directory)
        copies = [i for i, name in names.items() if name == label and i not in reserved]
        slot = min(copies) if copies else pending.get(key)
        if slot is None or slot in reserved or (slot in names and names[slot] != label):
            slot = next((i for i in range(1,129) if i not in names and i not in reserved), None)
        if slot is None:
            raise ValueError("No empty storage position available on the Floor")
        db.execute("INSERT INTO pending_ir VALUES(?,?) ON CONFLICT(id) DO UPDATE SET slot=excluded.slot", (key,slot))
    return {"op":"ir-install", "capture":key, "captureSlot":slot, "captureLabel":label, "sampleBytes":samples.hex()}
