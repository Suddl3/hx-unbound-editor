"""Local Helix USB bridge for preset reads, explicit recall and unsaved stock-boost edits."""
import argparse
import hashlib
import release_policy
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time
import webbrowser
from urllib.parse import unquote
from floor_nam_navigation import LEGACY_LABELS, read_slot_labels
from floor_boost import boost_identity, edit_boost, edit_revision
from floor_control_session import ControlSession, placement_catalog
from floor_visual_catalog import installed_resources, visual_catalog


ROOT = Path(__file__).resolve().parent
READER = ROOT / "build/floor-reader/tonepush.exe"
READER_SHA256 = "60e415f583d65b34b472ea7b56891c98c962beaef195bdc24bdfad0f0431f25c"
ADDRESS = ("127.0.0.1", 48736)
HOST = f"{ADDRESS[0]}:{ADDRESS[1]}"
LOCK = threading.Lock()

def library_command(commands):
    if hashlib.sha256(READER.read_bytes()).hexdigest() != READER_SHA256:
        raise ValueError("Unrecognized Floor reader.")
    result = subprocess.run([str(READER), "batch"], input=commands, capture_output=True,
                            text=True, encoding="utf-8", errors="replace", timeout=35,
                            creationflags=subprocess.CREATE_NO_WINDOW)
    if result.returncode or result.stderr.strip():
        raise ValueError("Floor library unavailable. Close HX Edit and check USB.")
    return result.stdout


def preset_directory(setlist):
    out = library_command(f"setlists\npresets --setlist {setlist}\n")
    lists = dict(re.findall(r"^([0-7])  (.+)$", out, re.M))
    presets = {k:v.removesuffix("<- loaded").strip() for k,v in re.findall(r"^(\d\d[A-D])[ \t]+([^\n]*)$", out, re.M)}
    if str(setlist) not in lists or not presets:
        raise ValueError("The Floor returned an incomplete preset list.")
    return {"setlists": lists, "presets": presets}


def nam_directory():
    from capture_library import Library
    db = ROOT / "build/capture-browser/library.sqlite3"
    library = Library(db.as_uri() + "?mode=ro", readonly=True)
    try:
        return [{"id": r["id"], "name": r["name"], "kind": r["type"], "ready": bool(r["ready"])}
                for r in library.rows()]
    finally:
        library.db.close()

def resolve_directory(text):
    library = ROOT / "build/capture-browser/library.sqlite3"
    db = sqlite3.connect(library.as_uri() + "?mode=ro", uri=True)
    try:
        return read_slot_labels(text, [row[0] for row in db.execute("SELECT id FROM captures WHERE wav IS NOT NULL")])
    finally:
        db.close()

def read_floor(instruction=""):
    if hashlib.sha256(READER.read_bytes()).hexdigest() != READER_SHA256:
        raise ValueError("The local TonePush reader differs from the verified 0.12.1 release.")
    # A separate process owns one USB session and tears it down before returning.
    with tempfile.TemporaryDirectory() as tmp:
        result = subprocess.run([str(READER), "batch"], cwd=tmp,
            input="irs\nchain\n" + instruction + "controllers\ntopology\nexport current.json\nchain\n",
            capture_output=True, encoding="utf-8", errors="replace",
            timeout=35 if instruction.startswith(("clear ", "model ")) else 15,
            creationflags=subprocess.CREATE_NO_WINDOW)
        out = result.stdout
        if result.returncode or not (Path(tmp) / "current.json").is_file():
            raise ValueError("Could not read the Floor. Check USB and keep HX Edit closed.")
        headers = list(re.finditer(r"^([0-9]{2}[A-D]) ([^\r\n]+?)[ \t]+[\d.]+ BPM[ \t]*$", out, re.M))
        diagnostics = result.stderr.replace("(install HX Edit for model and parameter names)", "").strip()
        if len(headers) != 2 or headers[0].groups() != headers[1].groups() or diagnostics:
            raise ValueError("The active preset could not be confirmed. Read the Floor again.")
        label, name = headers[0].groups()
        doc = json.loads((Path(tmp) / "current.json").read_text(encoding="utf-8"))
    lanes = ["\n".join(line.strip() for line in group.splitlines())
             for group in re.findall(r"^path \d+[ \t]*\n((?:[ \t]+[^\n]+\n?)+)", out, re.M)]
    blocks = doc["blocks"]
    slots = resolve_directory(out[:headers[0].start()])
    view = {"label": label, "name": name, "firmware": doc["firmware"], "blocks": blocks,
            "lanes": [s.strip() for s in lanes], "slots": slots, "slotSource": "floor-ir-directory",
            "cc20": bool(re.search(r"^block 1 Param\(0\)\s+MIDI CC 20\s+100%\.\.12800%\s*$", out, re.M))}
    view["boostRevision"] = edit_revision(view)
    return view


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def reply(self, status, content_type, data):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Frame-Options", "DENY")
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.headers.get('Host') == HOST and self.path == '/live/capabilities':
            payload = {**release_policy.capabilities(getattr(self.server, 'experimental_device', False)),
                       'instance': str(os.getpid())}
            self.reply(200, 'application/json', json.dumps(payload).encode())
            return
        if self.path in ('/transfer', '/current', '/nam', '/irs') or self.path.startswith('/presets/'):
            self.reply(403, 'application/json', json.dumps({'error': release_policy.TRANSFER_MESSAGE}).encode())
            return
        if self.headers.get("Host") != HOST:
            self.send_error(403)
            return
        exported = re.fullmatch(r"/live/export/([a-f0-9-]{36})",self.path)
        if exported and not self.server.manual_session:
            if self.headers.get("X-Floor-Read")!="1":
                self.send_error(403)
                return
            try:
                self.reply(200,"application/octet-stream",self.server.controls.export_bytes(exported[1]))
            except ValueError:
                self.send_error(404)
            return
        if not self.server.manual_session and self.path in ("/live/nam","/live/configured-favorites"):
            if self.headers.get("X-Floor-Read") != "1":
                self.send_error(403)
                return
            try:
                if self.path=="/live/configured-favorites":
                    from floor_configured_blocks import rows
                    result=rows(self.server.controls.library_path)
                else:
                    result=self.server.controls.nam_library()
                self.reply(200, "application/json", json.dumps(result).encode())
            except (OSError, ValueError, sqlite3.Error, subprocess.SubprocessError) as exc:
                self.reply(503, "application/json", json.dumps({"error": str(exc)}).encode())
            return
        icon = re.fullmatch(r"/live/art/([0-9a-f]{24})\.png", self.path)
        if not self.server.manual_session and (self.path in ("/live/models", "/live/catalog") or icon):
            if not icon and self.headers.get("X-Floor-Read") != "1":
                self.send_error(403)
                return
            try:
                binary = self.server.controls.binary
                if not binary.is_file() and self.path in ('/live/models', '/live/catalog'):
                    data = [] if self.path == '/live/models' else {'categories': [], 'models': [],
                        'notice': 'Build the bridge and install HX Edit for the stock catalog. Local NAM browsing is available now.'}
                    self.reply(200, 'application/json', json.dumps(data).encode())
                    return
                placements = placement_catalog(binary, binary.stat().st_mtime_ns)
                if self.path == "/live/models":
                    self.reply(200, "application/json", placements)
                    return
                metadata, art = visual_catalog(installed_resources(), placements)
                if icon:
                    token = icon[1]
                    if token not in art.files:
                        self.send_error(404)
                        return
                    body = art.read(token)
                    self.send_response(200)
                    self.send_header("Content-Type", "image/png")
                    self.send_header("X-Content-Type-Options", "nosniff")
                    self.send_header("Cache-Control", "private, max-age=86400")
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                else:
                    self.reply(200, "application/json", metadata)
            except (OSError, ValueError, subprocess.SubprocessError) as exc:
                self.reply(503, "application/json", json.dumps({"error": str(exc)}).encode())
            return
        if not self.server.manual_session and self.path in ("/", "/live", "/live?offline=1", "/live/state"):
            self.server.last_use = time.monotonic()
            if self.path != "/live/state":
                self.reply(200, "text/html; charset=utf-8", (ROOT / "design/live-controls.html").read_bytes())
            elif self.headers.get("X-Floor-Read") == "1":
                snapshot = self.server.controls.snapshot()
                version = f"{snapshot.get('session', '')}/{snapshot['version']}"
                if self.headers.get("X-Floor-Version") == version:
                    self.reply(204, "application/json", b"")
                else:
                    self.reply(200, "application/json", json.dumps(snapshot).encode())
            else:
                self.send_error(403)
            return
        if self.path == "/" or (self.path == "/transfer" and not self.server.manual_session):
            self.server.last_use = time.monotonic()
            page = (ROOT / "pedalboard-preview.html").read_bytes()
            if self.server.manual_session:
                page = page.replace(b"<head>", b'<head><meta name="floor-session" content="manual">', 1)
                if getattr(self.server, "batch_boost_move", False):
                    page = page.replace(b"<head>", b'<head><meta name="floor-boost-trial" content="three-sessions">', 1)
            self.reply(200, "text/html; charset=utf-8", page)
            return
        assets = {
            "/design/live-snapshots.js": "text/javascript",
            "/design/live-controllers.js": "text/javascript",
            "/design/live-controllers.css": "text/css",
            "/design/live-block-move.js": "text/javascript",
            "/design/preset-save.js": "text/javascript",
            "/design/preset-files.js": "text/javascript",
            "/design/backup-browser.js": "text/javascript",
            "/design/command-center.js": "text/javascript",
            "/design/live-routing.js": "text/javascript",
            "/design/configured-favorites.js": "text/javascript",
            "/design/live-model-browser.js": "text/javascript",
            "/design/live-model-browser.css": "text/css",
            "/design/live-controls.js": "text/javascript",
            "/design/live-controls.css": "text/css",
            "/design/gear-diagrams.js": "text/javascript",
            "/design/swap-theme.js": "text/javascript",
            "/design/swap-theme.css": "text/css",
            "/design/live-swap.css": "text/css",
            "/design/signal-path.js": "text/javascript",
            "/design/signal-path.css": "text/css",
            "/design/floor-librarian.js": "text/javascript",
        }
        if self.path in assets:
            self.reply(200, assets[self.path] + "; charset=utf-8",
                       (ROOT / self.path[1:]).read_bytes())
            return
        progress = re.fullmatch(r"/boost-progress/([a-f0-9]{32})", self.path)
        if progress or self.path == "/last-boost-move":
            if self.headers.get("X-Floor-Progress") != "1" or self.headers.get("Origin", f"http://{HOST}") != f"http://{HOST}":
                self.send_error(403)
                return
            if self.path == "/last-boost-move":
                self.reply(200, "application/json", json.dumps({"move": getattr(self.server, "last_move_timing", None)}).encode())
                return
            ticket, stage = getattr(self.server, "boost_progress", (None, None))
            self.reply(200, "application/json", json.dumps({"stage": stage if ticket == progress[1] else None}).encode())
            return
        bank = re.fullmatch(r"/presets/([0-7])", self.path)
        if (self.path not in ("/current","/nam","/irs") and not bank) or self.headers.get("X-Floor-Read") != "1":
            self.send_error(404)
            return
        if self.headers.get("Origin", f"http://{HOST}") != f"http://{HOST}":
            self.send_error(403)
            return
        self.server.last_use = time.monotonic()
        if self.server.manual_session and (self.path != "/current" or self.headers.get("X-Floor-Manual") != "1"):
            self.reply(409, "application/json", b'{"error":"USB follow paused for the short-session test."}')
            return
        if self.path == "/nam":
            try:
                self.reply(200, "application/json", json.dumps(nam_directory()).encode())
            except (OSError, ValueError, sqlite3.Error) as exc:
                self.reply(503, "application/json", json.dumps({"error":str(exc)}).encode())
            return
        requested = time.monotonic()
        if not LOCK.acquire(timeout=25 if bank or self.path == "/irs" else 15):
            self.reply(409, "application/json", b'{"error":"Another Floor read is in progress."}')
            return
        try:
            cached = getattr(self.server, "last_snapshot", None)
            if (self.path == "/current" and cached and self.headers.get("X-Floor-Fresh") != "1"
                    and cached[0] >= requested - 0.75):
                data = cached[1]
            elif bank:
                data = preset_directory(int(bank[1]))
            elif self.path == "/irs":
                data = resolve_directory(library_command("irs\n"))
            else:
                data = read_floor()
                self.server.last_snapshot = (time.monotonic(), data)
            if self.path == "/current":
                owner = getattr(self.server,"boost_owner",None)
                data["boostOwned"] = bool(owner and owner == boost_identity(data))
            status = 200
        except (OSError, ValueError, KeyError, sqlite3.Error, subprocess.SubprocessError) as error:
            data = {"error": str(error)}
            status = 503
        finally:
            LOCK.release()
        self.reply(status, "application/json", json.dumps(data).encode())

    def do_POST(self):
        if self.path == '/live/ir/install' or not self.path.startswith('/live/'):
            self.reply(403, 'application/json', json.dumps({'error': release_policy.TRANSFER_MESSAGE}).encode())
            return
        if self.path in ("/live/inspect-file","/live/inspect-backup") and not self.server.manual_session:
            if (self.headers.get("Host")!=HOST or self.headers.get("Origin")!=f"http://{HOST}"
                    or self.headers.get("Content-Type")!="application/octet-stream"):
                self.send_error(403)
                return
            try:
                length=int(self.headers.get("Content-Length","0"))
                backup=self.path=="/live/inspect-backup"
                limit=(16 if backup else 1)*1024*1024
                if not 1<=length<=limit: raise ValueError(f"Choose a file up to {limit//(1024*1024)} MiB.")
                raw=self.rfile.read(length)
                if len(raw)!=length: raise ValueError("Incomplete preset file.")
                mode="--inspect-backup" if backup else "--inspect-preset"
                probe=subprocess.run([self.server.controls.binary,mode,"-"],input=raw,capture_output=True,
                                     timeout=20 if backup else 8,creationflags=subprocess.CREATE_NO_WINDOW)
                if probe.returncode: raise ValueError("Choose a supported Floor .hxb backup." if backup else "Choose a native dual-DSP Floor 3.80 .hxpreset file.")
                self.reply(200,"application/json",probe.stdout)
            except (ValueError,OSError,subprocess.SubprocessError) as error:
                self.reply(400,"application/json",json.dumps({"error":str(error)}).encode())
            return
        if self.path == "/live/ir/install" and not self.server.manual_session:
            if (self.headers.get("Host") != HOST or self.headers.get("Origin") != f"http://{HOST}"
                    or self.headers.get("Content-Type") != "application/octet-stream"):
                self.send_error(403)
                return
            try:
                size = int(self.headers.get("Content-Length", "0"))
                if not 12 <= size <= 1024 * 1024:
                    raise ValueError("Choose a short mono impulse WAV (up to 2,048 samples).")
                raw = self.rfile.read(size)
                if len(raw) != size:
                    raise ValueError("Incomplete WAV upload")
                name = unquote(self.headers.get("X-IR-Name",""))
                if not name or len(name) > 255 or Path(name).name != name or any(ord(c)<32 for c in name):
                    raise ValueError("Invalid WAV filename")
                self.server.controls.edit({"op":"ir-install", "session":self.headers.get("X-Floor-Session"),
                                           "rev":int(self.headers.get("X-Floor-Rev","-1"))},
                                          impulse=(name,self.headers.get("X-IR-Kind"),raw))
                self.reply(200, "application/json", b"{}")
            except (OSError, ValueError, sqlite3.Error, subprocess.SubprocessError) as exc:
                self.reply(409, "application/json", json.dumps({"error":str(exc)}).encode())
            return
        if self.path == "/live/nam/classify" and not self.server.manual_session:
            if (self.headers.get("Host") != HOST or self.headers.get("Origin") != f"http://{HOST}"
                    or self.headers.get("Content-Type") != "application/json"):
                self.send_error(403)
                return
            try:
                from floor_nam_navigation import classify_capture
                size = int(self.headers.get("Content-Length", "0"))
                if not 2 <= size <= 1024:
                    raise ValueError("Invalid classification request")
                body = json.loads(self.rfile.read(size))
                classify_capture(self.server.controls.library_path, body["capture"],
                                 kind=body["kind"], instrument=body["instrument"])
                self.reply(200, "application/json", b"{}")
            except (ValueError, KeyError, TypeError, OSError, sqlite3.Error) as exc:
                self.reply(400, "application/json", json.dumps({"error":str(exc)}).encode())
            return
        if self.path == "/live/nam/import" and not self.server.manual_session:
            if (self.headers.get("Host") != HOST or self.headers.get("Origin") != f"http://{HOST}"
                    or self.headers.get("Content-Type") != "application/octet-stream"):
                self.send_error(403)
                return
            try:
                filename = unquote(self.headers.get("X-Capture-Filename",""))
                if (not filename.lower().endswith(".nam") or len(filename) > 255 or
                        Path(filename).name != filename or any(ord(c) < 32 for c in filename)):
                    raise ValueError("Choose a .nam profile")
                length = int(self.headers.get("Content-Length","0"))
                if not 0 < length <= 32 * 1024 * 1024:
                    raise ValueError("Choose a .nam profile up to 32 MiB")
                raw = self.rfile.read(length)
                if len(raw) != length:
                    raise ValueError("Incomplete capture upload")
                from capture_library import Library
                library = Library(self.server.controls.library_path)
                try:
                    ids, missing = library.import_nam(filename,raw)
                finally:
                    library.db.close()
                self.reply(200, "application/json", json.dumps({"ids":sorted(ids),"missing":missing}).encode())
            except (ValueError, KeyError, TypeError, OverflowError, RecursionError, sqlite3.Error, OSError) as exc:
                self.reply(400, "application/json", json.dumps({"error":str(exc)}).encode())
            return
        if not self.server.manual_session and self.path in ("/live/connect", "/live/edit", "/live/release", "/live/incident", "/live/shutdown"):
            if (self.headers.get("Host") != HOST or self.headers.get("Origin") != f"http://{HOST}"
                    or self.headers.get("Content-Type") != "application/json"):
                self.send_error(403)
                return
            try:
                size = int(self.headers.get("Content-Length", "-1"))
                if not 2 <= size <= 1024:
                    raise ValueError("Invalid request size")
                body = json.loads(self.rfile.read(size))
                if not isinstance(body, dict):
                    raise ValueError("Expected an edit object")
                if self.path == '/live/shutdown':
                    self.server.controls.stop()
                    self.server.stop_requested = True
                elif self.path == "/live/connect":
                    if not getattr(self.server, 'experimental_device', False):
                        raise ValueError(release_policy.DEVICE_MESSAGE)
                    self.server.controls.start(manual=body.get("manual") is True)
                elif self.path == "/live/edit":
                    release_policy.require_edit(getattr(self.server, 'experimental_device', False), body)
                    self.server.controls.edit(body)
                elif self.path == "/live/incident":
                    self.server.controls.report_incident(body)
                else:
                    self.server.controls.stop()
                self.server.last_use = time.monotonic()
                self.reply(200, "application/json", b'{}')
            except (OSError, ValueError, subprocess.SubprocessError) as exc:
                self.reply(409, "application/json", json.dumps({"error": str(exc)}).encode())
            return
        target = re.fullmatch(r"/recall/([0-7])/(0[1-9]|[12]\d|3[0-2])([A-D])", self.path)
        boost = re.fullmatch(r"/boost/(add|remove|on|off|move|use)/([1-8])", self.path)
        if self.server.manual_session and (not boost or self.headers.get("X-Floor-Manual") != "1"):
            self.send_error(403)
            return
        if (self.headers.get("Host") != HOST or self.headers.get("Origin") != f"http://{HOST}"
                or not ((target and self.headers.get("X-Floor-Recall") == "1")
                        or (boost and re.fullmatch("[0-9a-f]{64}", self.headers.get("X-Floor-Revision",""))))):
            self.send_error(403)
            return
        if not LOCK.acquire(blocking=False):
            self.reply(409, "application/json", b'{"error":"Floor busy. Try recall again."}')
            return
        timing = None
        def finish_move(outcome):
            if timing is not None:
                timing.update(outcome=outcome, seconds=round(time.monotonic()-started, 3))
                self.server.last_move_timing = timing
        try:
            self.server.last_snapshot = None
            if boost:
                action, position = boost[1], int(boost[2])
                if action == "move":
                    started = time.monotonic()
                    timing = {"position":position, "startedUTC":time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "steps":[]}
                def measure(label, operation, argument):
                    if timing is None:
                        return operation(argument)
                    tick = time.monotonic()
                    try:
                        return operation(argument)
                    finally:
                        timing["steps"].append({"label":label,"seconds":round(time.monotonic()-tick,3)})
                next_read = "Check preset and positions"
                def command(instruction):
                    nonlocal next_read
                    removing = instruction.startswith("clear ")
                    next_read = "Verify removal" if removing else "Verify placement"
                    return measure("Remove boost" if removing else "Place boost", library_command, instruction)
                ticket = self.headers.get("X-Floor-Operation", "")
                def report(stage):
                    if re.fullmatch("[a-f0-9]{32}", ticket):
                        self.server.boost_progress = (ticket, stage)
                report("Checking the current preset and destination")
                def read_target(instruction=""):
                    step = next_read
                    if instruction.startswith("clear "):
                        step = "Remove boost and verify removal"
                    elif instruction.startswith("model "):
                        step = "Place boost and verify placement"
                    view = measure(step, read_floor, instruction)
                    if self.server.manual_session and view["label"] != "07B":
                        raise ValueError("Recall spare preset 07B before editing the boost.")
                    return view
                before = read_target()
                key = boost_identity(before)
                owned = bool(key and key == getattr(self.server,"boost_owner",None))
                current = next((b for b in before["blocks"] if b["position"] == position+1),None)
                revision = self.headers["X-Floor-Revision"]
                if revision != before["boostRevision"] or (action not in ("add","use") and not owned):
                    before["boostOwned"] = owned
                    finish_move("Stopped at preflight")
                    response = {"error":"Floor state refreshed. Choose the next action.","view":before}
                    if timing is not None:
                        response["moveTiming"] = timing
                    self.reply(409, "application/json", json.dumps(response).encode())
                    return
                if action == "use":
                    if not self.server.manual_session or not key or not current or current["model"] != "model 388":
                        raise ValueError("Select the existing Kinky Boost on spare preset 07B.")
                    view = before
                else:
                    batch_move = action == "move" and self.server.manual_session and getattr(self.server, "batch_boost_move", False)
                    view = edit_boost(action, position, revision, read_target, command,
                                      before=before, write_and_read=read_target if batch_move or action in ("on","off") else None,
                                      report=report)
                self.server.boost_owner = boost_identity(view)
                view["boostOwned"] = bool(self.server.boost_owner)
                self.server.last_snapshot = (time.monotonic(), view)
                finish_move("Confirmed")
                response = {**view, "moveTiming":timing} if timing is not None else view
                self.reply(200, "application/json", json.dumps(response).encode())
                return
            self.server.boost_owner = None
            library_command(f"select {target[2]}{target[3]} --setlist {target[1]}\n")
            self.reply(200, "application/json", b'{"requested":true}')
        except (OSError, ValueError, subprocess.SubprocessError) as exc:
            if timing is not None and "outcome" not in timing:
                finish_move("Stopped before confirmation")
            response = {"error":str(exc)}
            if timing is not None:
                response["moveTiming"] = timing
            self.reply(503, "application/json", json.dumps(response).encode())
        finally:
            self.server.boost_progress = (None, None)
            LOCK.release()


def open_editor():
    url = f"http://{HOST}/" if "--manual-session" in sys.argv else f"http://{HOST}/live?offline=1"
    edge = Path(os.environ.get("ProgramFiles(x86)", "")) / "Microsoft/Edge/Application/msedge.exe"
    if edge.is_file():
        subprocess.Popen([str(edge), url])
    else:
        webbrowser.open(url)


def main():
    global ADDRESS, HOST
    parser = argparse.ArgumentParser(description='HX Unbound experimental source preview')
    parser.add_argument('--port', type=int, default=48736)
    parser.add_argument('--no-browser', action='store_true')
    parser.add_argument('--experimental-device', action='store_true')
    args = parser.parse_args()
    if not 1024 <= args.port <= 65535:
        parser.error('port must be between 1024 and 65535')
    ADDRESS = ('127.0.0.1', args.port)
    HOST = f'{ADDRESS[0]}:{ADDRESS[1]}'
    from capture_library import Library
    library_path = ROOT / 'build/capture-browser/library.sqlite3'
    library_path.parent.mkdir(parents=True, exist_ok=True)
    (ROOT / 'build/floor-session').mkdir(parents=True, exist_ok=True)
    Library(library_path).db.close()
    try:
        server = ThreadingHTTPServer(ADDRESS, Handler)
    except OSError as error:
        if getattr(error, "winerror", None) != 10048:
            raise
        # Only reopen an existing instance of this editor.
        from urllib.request import urlopen
        with urlopen(f"http://{HOST}/", timeout=3) as page:
            html = page.read(65536)
            if b'id="floorRead"' not in html and b'id="libraryTabs"' not in html:
                raise ValueError("The HX Unbound port is already in use by another app.")
            if "--manual-session" in sys.argv and b'name="floor-session" content="manual"' not in html:
                raise ValueError("The running editor is in follow mode. Stop its bridge before launching the short-session test.")
            if "--manual-session" not in sys.argv and b'name="floor-session" content="manual"' in html:
                raise ValueError("Close the short-session bridge before launching the live editor.")
            trial_requested = "--batch-boost-move" in sys.argv and "--manual-session" in sys.argv
            if trial_requested != (b'name="floor-boost-trial" content="three-sessions"' in html):
                raise ValueError("The running bridge uses a different boost move mode. Close it before switching tests.")
        if "--no-browser" not in sys.argv:
            open_editor()
        return
    with server:
        server.manual_session = False
        server.experimental_device = args.experimental_device
        server.stop_requested = False
        server.controls = ControlSession(ROOT, LOCK)
        server.batch_boost_move = server.manual_session and "--batch-boost-move" in sys.argv
        server.timeout = 2
        server.last_use = time.monotonic()
        if "--no-browser" not in sys.argv:
            open_editor()
        while not server.stop_requested and time.monotonic() - server.last_use < 1800:
            server.handle_request()
        server.controls.stop()


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"HX Unbound: {error}", file=sys.stderr)
        raise SystemExit(1)
