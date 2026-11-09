"""Reserve an empty device slot before a user-requested capture installation."""
import re
import sqlite3
from contextlib import closing
from floor_nam_navigation import capture_payload, resolve_label
from floor_slot_labels import floor_label


def prepare_install(path, directory, key):
    if not isinstance(key, str) or not re.fullmatch("[0-9a-f]{64}", key):
        raise ValueError("Choose a local capture")
    raw = capture_payload(path, key)
    with closing(sqlite3.connect(path)) as db, db:
        db.execute("BEGIN IMMEDIATE")
        db.execute("CREATE TABLE IF NOT EXISTS pending_ir(id TEXT PRIMARY KEY,slot INTEGER UNIQUE NOT NULL)")
        captures = dict(db.execute("SELECT id,name FROM captures WHERE wav IS NOT NULL"))
        if sum(other.startswith(key[:8]) for other in captures) != 1:
            raise ValueError("Capture identifier is ambiguous")
        names = dict(directory)
        matches = [slot for slot,name in names.items() if resolve_label(name, captures) == key]
        if len(matches) > 1:
            raise ValueError("More than one installed copy matches this capture")
        pending = dict(db.execute("SELECT id,slot FROM pending_ir"))
        reserved = {slot for ident,slot in pending.items() if ident != key}
        reserved.update(slot for ident,slot in db.execute("SELECT id,slot FROM captures WHERE slot IS NOT NULL") if ident != key)
        if matches:
            slot = matches[0]
            label = names[slot]
        else:
            slot = pending.get(key)
            if slot is None or slot in names or slot in reserved:
                slot = next((i for i in range(1,129) if i not in names and i not in reserved),None)
            if slot is None:
                raise ValueError("No empty storage position available on the Floor")
            label = floor_label("NAM", captures[key], key)
        if slot in reserved:
            raise ValueError("Another capture has reserved this storage position")
        db.execute("INSERT INTO pending_ir(id,slot) VALUES(?,?) ON CONFLICT(id) DO UPDATE SET slot=excluded.slot",(key,slot))
    return {"op":"nam-install","capture":key,"captureSlot":slot,"captureLabel":label,"sampleBytes":raw.hex()}


def confirm_install(path, key, slot):
    with closing(sqlite3.connect(path)) as db, db:
        db.execute("BEGIN IMMEDIATE")
        if db.execute("SELECT slot FROM pending_ir WHERE id=?", (key,)).fetchone() != (slot,):
            raise ValueError("Capture installed. Its local reservation changed.")
        db.execute("UPDATE captures SET slot=? WHERE id=?", (slot,key))
        db.execute("DELETE FROM pending_ir WHERE id=?", (key,))

