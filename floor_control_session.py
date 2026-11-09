"""Own the persistent USB helper while the live controls page is in use."""
import json
import subprocess
import sqlite3
import threading
import time
import uuid
from collections import deque
from datetime import datetime, timezone
import floor_configured_blocks as configured
from pathlib import Path
from functools import lru_cache
from floor_nam_navigation import nam_rows, prepare_selection, name_live_blocks
from floor_nam_install import prepare_install, confirm_install
from floor_ir_import import prepare_ir_install
from contextlib import closing


@lru_cache(maxsize=1)
def placement_catalog(binary, modified):
    result = subprocess.run([binary, "--placement-catalog"], capture_output=True, check=True,
                            timeout=8, creationflags=subprocess.CREATE_NO_WINDOW)
    rows = json.loads(result.stdout)
    if not isinstance(rows, list) or not rows:
        raise ValueError("Stock model catalog unavailable")
    return result.stdout


class ControlSession:
    def __init__(self, root: Path, usb_lock):
        self.binary = root / "build/floor-session/target/release/floor-editor.exe"
        self.library_path = root / "build/capture-browser/library.sqlite3"
        self.usb_lock = usb_lock
        self.lock = threading.Lock()
        self.process = None
        self.installing = None
        self.saving = None
        self.last_export = None
        self.version = 0
        self.state = {"online": False, "message": "Disconnected", "pending": False}
        self.last_seen = 0
        self.trace = deque(maxlen=48)
        self.diagnostics_dir = self.binary.parents[2]
        self.recovery_marker = self.diagnostics_dir / "usb-session-interrupted"
        self.recovery_required = self.recovery_marker.exists()
        # Recognize the last interrupted session from older builds on upgrade.
        previous = self.diagnostics_dir / "live-editor-errors.log"
        if previous.is_file() and not self.recovery_required:
            self.recovery_required = self._partial_transfer(self._last_error(previous))
        if self.recovery_required:
            self.state.update(recoveryRequired=True, message=self.recovery_message())

    @staticmethod
    def _last_error(path):
        try:
            with path.open("rb") as stream:
                stream.seek(0, 2)
                stream.seek(max(0, stream.tell() - 1024))
                return stream.read().decode("utf-8", errors="replace").strip()
        except OSError:
            return ""

    @staticmethod
    def _partial_transfer(error):
        return "transfer failed part-way" in error.lower() or "write timed out" in error.lower()

    @staticmethod
    def recovery_message():
        return ("USB transfer interrupted. Check the last edit on the Floor. If its panel is frozen, "
                "unplug USB-B and check it again. Reconnect USB and click Reconnect Floor when ready.")

    def _observe(self, phase, **fields):
        self.trace.append({"utc":datetime.now(timezone.utc).isoformat(timespec="milliseconds"),
                           "phase":phase, **fields})

    def _record_incident(self, error, observation=None):
        board = self.state.get("board") or {}
        row = {"utc":datetime.now(timezone.utc).isoformat(timespec="milliseconds"),
               "kind":"player report" if observation else "helper exit",
               "session":self.state.get("session"), "preset":board.get("label"),
               "setlist":board.get("bank"), "snapshotIndex":(board.get("snapshots") or {}).get("active"),
               "error":error[-1024:], "recent":list(self.trace)}
        if observation:
            row.update(observation=observation, online=self.state.get("online"), pending=self.state.get("pending"),
                       bridgeMessage=self.state.get("message"))
        path = self.diagnostics_dir / "usb-incidents.jsonl"
        try:
            if path.exists() and path.stat().st_size >= 1024*1024:
                path.replace(path.with_suffix(".previous.jsonl"))
            with path.open("a", encoding="utf-8") as log:
                log.write(json.dumps(row, ensure_ascii=True)+"\n")
            return True
        except OSError:
            return False  # Diagnostic storage must never stop USB cleanup.

    def report_incident(self, body):
        panel, audio, action = body.get("panel"), body.get("audio"), body.get("action","")
        if (panel not in ("unknown","responsive","frozen") or audio not in ("unknown","playing","interrupted")
                or not isinstance(action,str) or len(action)>180):
            raise ValueError("Choose the observed symptoms and keep the action under 181 characters.")
        with self.lock:
            if not self._record_incident("", {"panel":panel,"audio":audio,"action":action.strip()}):
                raise ValueError("Could not write the local incident log.")

    def snapshot(self):
        with self.lock:
            self.last_seen = time.monotonic()
            return {**self.state, "version": self.version}

    def nam_library(self):
        with self.lock:
            self.last_seen = time.monotonic()
            board = self.state.get("board") or {}
            directory = board.get("captureDirectory", []) if self.state["online"] else []
        return nam_rows(self.library_path, directory)

    def export_bytes(self, request):
        with self.lock:
            if not self.last_export or self.last_export[0]!=request:
                raise ValueError("This export expired. Read the preset again.")
            return self.last_export[1]

    def start(self, manual=False):
        with self.lock:
            self.last_seen = time.monotonic()
            if self.process:
                return
            if self.recovery_required and not manual:
                raise ValueError(self.recovery_message())
            if not self.usb_lock.acquire(blocking=False):
                raise ValueError("Another editor operation is using USB. Try again when it finishes.")
            log = self.binary.parents[2] / "live-editor-errors.log"
            try:
                with log.open("w", encoding="utf-8") as errors:
                    proc = subprocess.Popen([self.binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                            stderr=errors, encoding="utf-8", creationflags=subprocess.CREATE_NO_WINDOW)
            except Exception:
                self.usb_lock.release()
                raise
            self.process = proc
            self.trace.clear()
            self._observe("session started")
            self.last_export = None
            self.version += 1
            self.state = {"session": uuid.uuid4().hex, "online": False, "pending": False, "message": "Connecting"}
            threading.Thread(target=self._read, args=(proc,log), daemon=True).start()
            threading.Thread(target=self._expire, args=(proc,), daemon=True).start()

    def _expire(self, proc):
        while proc.poll() is None:
            time.sleep(2)
            with self.lock:
                if self.process is not proc:
                    return
                if self.state["pending"] or self.installing is not None or self.saving is not None or time.monotonic() - self.last_seen < 25:
                    continue
            self.stop(proc)
            return

    def _read(self, proc, log):
        message = "USB released"
        configured_status = None
        try:
            for line in proc.stdout:
                event = json.loads(line)
                if "usbPhase" in event:
                    with self.lock:
                        details = {k:event[k] for k in ("elapsedMs","channels") if k in event}
                        self._observe(event["usbPhase"], **details)
                    continue
                if "configuredBlock" in event:
                    helper_message = event.get("message")
                    configured.receive(self.library_path, event)
                    configured_status = (helper_message, event["message"])
                if event.get("board"):
                    if configured_status is not None:
                        helper_message, local_message = configured_status
                        if event.get("message") == helper_message:
                            event["message"] = local_message
                        configured_status = None
                    try:
                        name_live_blocks(self.library_path, event["board"])
                    except (sqlite3.Error, OSError, ValueError):
                        pass  # The Floor’s own block names are still available.
                with self.lock:
                    if event.get("board") and self.recovery_required:
                        self.recovery_required=False
                        try:
                            self.recovery_marker.unlink(missing_ok=True)
                        except OSError:
                            pass
                    if event.get("board") and self.state["pending"] and event.get("pending") is False:
                        self._observe("helper finished", outcome=event.get("message","")[:160])
                    elif event.get("board") and event.get("message")=="Received from Floor":
                        self._observe("Floor update", revision=event.get("rev"))
                    file=event.get("presetFile")
                    if file:
                        self.last_export=(file["request"],bytes.fromhex(file.pop("hex")))
                    if self.saving is not None and event.get("presetSave",{}).get("request")==self.saving:
                        self.saving=None
                    if "installFinished" in event and event["installFinished"] == self.installing:
                        if "installedSlot" in event:
                            try:
                                if self.installing.startswith("ir:"):
                                    with closing(sqlite3.connect(self.library_path)) as db, db:
                                        db.execute("DELETE FROM pending_ir WHERE id=? AND slot=?",
                                                   (self.installing, event["installedSlot"]))
                                else:
                                    confirm_install(self.library_path, self.installing, event["installedSlot"])
                            except (sqlite3.Error, ValueError) as error:
                                event["message"] = f"Floor verified the installation. Local reservation: {error}"
                        self.installing = None
                    self.version += 1
                    self.state.update(event, online=True)
            if proc.wait(timeout=3):
                message = self._last_error(log) or "USB session ended unexpectedly"
        except (ValueError, OSError, subprocess.TimeoutExpired) as error:
            message = str(error)
        finally:
            if proc.poll() is None:
                proc.kill()
                proc.wait(timeout=3)
            with self.lock:
                self._observe("session ended")
                if message != "USB released":
                    self._record_incident(message)
                if self._partial_transfer(message):
                    self.recovery_required = True
                    try:
                        self.recovery_marker.touch()
                    except OSError:
                        pass
                    message = self.recovery_message()
                self.process = None
                self.installing = None
                self.saving = None
                self.version += 1
                self.state.update(online=False, pending=False, recoveryRequired=self.recovery_required, message=message)
                self.usb_lock.release()

    def stop(self, expected=None):
        with self.lock:
            proc = self.process
            if not proc or (expected is not None and proc is not expected):
                return
            if self.state["pending"] or self.installing is not None or self.saving is not None:
                if expected is not None:
                    return
                raise ValueError("An edit is in progress. Wait for Floor readback before disconnecting.")
            try:
                proc.stdin.write("stop\n")
                proc.stdin.flush()
            except OSError:
                pass
        try:
            proc.wait(timeout=4)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=3)

    def edit(self, cmd, *, impulse=None):
        with self.lock:
            if cmd.get("op") in ("snapshot-details","snapshot-copy-bypass"):
                raise ValueError("Use the Floor for snapshot names, colors and bypass copies while USB writes are under investigation.")
            state = self.state
            if (not self.process or not state["online"] or state["pending"] or self.installing is not None or self.saving is not None or
                cmd.get("session") != state["session"] or cmd.get("rev") != state.get("rev")):
                raise ValueError("Floor changed. Check the current block and try again.")
            if cmd.get("op") == "ir-install" and impulse is not None:
                board = state.get("board") or {}
                if "captureDirectory" not in board:
                    raise ValueError("Read the Floor before installing an impulse.")
                cmd = {**prepare_ir_install(self.library_path, board["captureDirectory"], *impulse), "rev":state["rev"]}
            elif cmd.get("op") not in ("bypass","param","presets","irs","inspect-slot","recall","add","replace","remove","nam","nam-install",
                                       "snapshot-select","snapshot-metadata","controller-param","controller-bypass","move-block","copy-block",
                                       "preset-export","preset-targets","preset-save-preview","preset-save-commit","configured-capture","configured-apply","route-endpoint"):
                raise ValueError("Unknown edit")
            if cmd["op"]=="configured-apply":
                cmd=configured.trusted_apply(self.library_path,cmd)
            if cmd["op"] == "nam":
                board = state.get("board", {})
                if not board.get("namReady"):
                    raise ValueError("Recall a preset containing NAM A2 Lite to identify the running engine.")
                target = next((b for b in board["blocks"] if b["slot"] == cmd.get("slot") and b.get("namTarget")), None)
                preferred = target["params"][0]["value"] if target and target["params"] else None
                cmd = {**cmd, **prepare_selection(self.library_path, board["captureDirectory"], cmd.get("capture"), preferred)}
            if cmd["op"] == "nam-install":
                board = state.get("board") or {}
                if not board.get("namReady") or "captureDirectory" not in board:
                    raise ValueError("Recall a preset containing NAM A2 Lite to identify the running engine.")
                install = prepare_install(self.library_path, board["captureDirectory"], cmd.get("capture"))
                cmd = {**install, "rev":state["rev"]}
            self.last_seen = time.monotonic()
            self._observe("command", op=cmd["op"], **{key:cmd[key] for key in ("rev","bank","slot","index","from","to") if isinstance(cmd.get(key),int)})
            self.process.stdin.write(json.dumps(cmd, allow_nan=False) + "\n")
            self.process.stdin.flush()
            if cmd["op"]=="preset-save-commit":
                self.saving=cmd.get("request")
            if cmd["op"] in ("nam-install", "ir-install"):
                self.installing = cmd["capture"]
            state.update(pending=True, message="Sending")
            self.version += 1

