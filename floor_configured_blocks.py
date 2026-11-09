"""Editor-local favorites store exact configured stock blocks read by the USB helper."""
from contextlib import closing
import hashlib
from pathlib import Path
import sqlite3


def rows(path):
    if not Path(path).is_file():
        return []
    with closing(sqlite3.connect(Path(path).resolve().as_uri() + "?mode=ro", uri=True)) as db:
        if not db.execute("SELECT 1 FROM sqlite_master WHERE name='configured_blocks' AND type='table'").fetchone():
            return []
        return [dict(id=identity,name=name,model=model) for identity,name,model in db.execute(
            "SELECT id,name,model FROM configured_blocks ORDER BY name COLLATE NOCASE,id")]


def fingerprint(name, data):
    return hashlib.sha256(name.encode("utf8") + b"\0" + data).hexdigest()


def receive(path, event):
    record=event.pop("configuredBlock",None)
    if record is None:
        return
    try:
        name=record["name"]
        raw=bytes.fromhex(record["hex"])
        if not 1 <= len(name) <= 48 or not 4 <= len(raw) <= 16384:
            raise ValueError("Invalid configured block record")
        identity=fingerprint(name,raw)
        Path(path).parent.mkdir(parents=True,exist_ok=True)
        with closing(sqlite3.connect(path)) as db, db:
            db.execute("CREATE TABLE IF NOT EXISTS configured_blocks("
                       "id TEXT PRIMARY KEY,name TEXT NOT NULL,model INTEGER NOT NULL,raw BLOB NOT NULL)")
            db.execute("INSERT OR IGNORE INTO configured_blocks VALUES(?,?,?,?)",(identity,name,record["model"],raw))
        event["configuredRevision"]=identity
        event["message"]=f"Saved configured favorite · {name}"
    except (OSError,sqlite3.Error,ValueError,TypeError,KeyError) as error:
        event["message"]=f"Could not save configured favorite · {error}"


def trusted_apply(path, cmd):
    identity=cmd.get("favorite")
    if not isinstance(identity,str) or len(identity)!=64:
        raise ValueError("Choose a saved configured favorite")
    with closing(sqlite3.connect(Path(path).resolve().as_uri()+"?mode=ro",uri=True)) as db:
        result=db.execute("SELECT name,model,raw FROM configured_blocks WHERE id=?",(identity,)).fetchone()
    if not result:
        raise ValueError("Configured favorite is missing")
    name,model,raw=result
    if not 4<=len(raw)<=16384 or fingerprint(name,raw)!=identity:
        raise ValueError("Configured favorite content changed")
    return {"op":"configured-apply","rev":cmd["rev"],"slot":cmd.get("slot"),"model":model,"blockHex":raw.hex()}
