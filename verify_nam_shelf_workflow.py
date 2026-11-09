"""Exercise the local library HTTP routes and install reservations without USB."""
import hashlib
import io
from types import SimpleNamespace
from contextlib import closing
import json
import sqlite3
import struct
import tempfile
import threading
from http.server import ThreadingHTTPServer
from pathlib import Path
from urllib.request import Request, urlopen
from urllib.error import HTTPError
import unittest

import floor_live_editor as editor
from floor_control_session import ControlSession
from floor_nam_install import confirm_install, prepare_install
from floor_ir_import import impulse_samples, prepare_ir_install


class ShelfWorkflow(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=editor.ROOT/"build")
        self.addCleanup(self.temp.cleanup)
        self.db = Path(self.temp.name)/"captures.sqlite3"
        self.raw = (editor.ROOT/"tests/synthetic-a2.json").read_bytes()
        self.capture = hashlib.sha256(self.raw).hexdigest()
        self.server = ThreadingHTTPServer(("127.0.0.1",0), editor.Handler)
        self.server.manual_session = False
        self.server.controls = ControlSession(editor.ROOT, threading.Lock())
        self.server.controls.library_path = self.db
        self.addCleanup(self.server.server_close)
        original_host = editor.HOST
        editor.HOST = "127.0.0.1:"+str(self.server.server_port)
        self.addCleanup(setattr,editor,"HOST",original_host)
        threading.Thread(target=self.server.serve_forever,daemon=True).start()
        self.addCleanup(self.server.shutdown)
        self.origin = "http://"+editor.HOST

    def request(self,path,body=None,headers=None):
        req = Request(self.origin+path,data=body,headers={"Origin":self.origin,"X-Floor-Read":"1",**(headers or {})})
        with urlopen(req,timeout=4) as response:
            return json.load(response)

    @unittest.skipUnless((editor.ROOT/"build/hx-edit-capture/recorded-floor-edit-buffer.hxpreset").is_file(),
                         "Requires original Floor capture")
    def test_offline_native_file_inspection(self):
        saved=(editor.ROOT/"build/hx-edit-capture/recorded-floor-edit-buffer.hxpreset").read_bytes()
        headers={"Content-Type":"application/octet-stream"}
        result=self.request("/live/inspect-file",saved,headers)
        self.assertEqual(0,result["snapshots"]["active"])
        self.assertEqual(8,len(result["snapshots"]["slots"]))
        self.assertEqual("Multi (Guitar, Aux, Variax)",result["paths"][0]["input"]["name"])
        self.assertNotIn("bank",result)
        with self.assertRaises(HTTPError) as invalid:
            self.request("/live/inspect-file",b"not a preset",headers)
        self.assertEqual(400,invalid.exception.code)
        with self.assertRaises(HTTPError) as cross_origin:
            self.request("/live/inspect-backup",b"data",{**headers,"Origin":"http://example.invalid"})
        self.assertEqual(403,cross_origin.exception.code)
        self.assertIsNone(self.server.controls.process)

    def test_compound_tags_through_import_override_and_reset(self):
        headers = {"Content-Type":"application/octet-stream","X-Capture-Filename":"Combo.nam"}
        expected = {}
        for gear, label in [("pedal_amp","Pedal + amp"), ("amp_pedal_cab","Pedal + amp + cab"),
                            ("preamp","Preamp"), ("studio","Studio")]:
            raw = json.dumps({"architecture":"LSTM","metadata":{"gear_type":gear}}).encode()
            key, = self.request("/live/nam/import",raw,headers)["ids"]
            expected[key] = label
        rows = self.request("/live/nam")
        self.assertEqual({r["id"]:r["kind"] for r in rows}, expected)
        self.assertTrue(all(r["kind"] == r["sourceKind"] and not r["ready"] for r in rows))
        key = rows[0]["id"]
        for override in ("Pedal + amp + cab", None):
            self.request("/live/nam/classify",json.dumps({"capture":key,"instrument":None,"kind":override}).encode(),
                         {"Content-Type":"application/json"})
            row = next(r for r in self.request("/live/nam") if r["id"] == key)
            self.assertEqual((row["kind"],row["sourceKind"]), (override or expected[key],expected[key]))
        self.assertIsNone(self.server.controls.process)

    def test_import_classify_and_reserve(self):
        headers = {"Content-Type":"application/octet-stream","X-Capture-Filename":"Listening%20test.nam"}
        with self.assertRaises(HTTPError) as denied:
            self.request("/live/nam/import",self.raw,{**headers,"Origin":"http://example.invalid"})
        self.assertEqual(denied.exception.code,403)
        for _ in range(2):
            self.assertEqual(self.request("/live/nam/import",self.raw,headers)["ids"],[self.capture])
        tag = {"capture":self.capture,"kind":"Pedal","instrument":"Bass"}
        self.request("/live/nam/classify",json.dumps(tag).encode(),{"Content-Type":"application/json"})
        row, = self.request("/live/nam")
        self.assertEqual((row["kind"],row["instrument"],row["installed"]),("Pedal","Bass",False))
        self.assertTrue(row["ready"])
        directory = [(1,"Occupied"),(2,"Other")]
        first = prepare_install(self.db,directory,self.capture)
        self.assertEqual(first["captureSlot"],3)
        self.assertTrue(first["captureLabel"].startswith("NAM "))
        self.assertLessEqual(len(first["captureLabel"].encode("ascii")),31)
        legacy_label = "Previous-" + self.capture[:8] + "-Lite"
        old = prepare_install(self.db,[*directory,(78,legacy_label)],self.capture)
        self.assertEqual((old["captureSlot"],old["captureLabel"]),(78,legacy_label))
        with closing(sqlite3.connect(self.db)) as db, db:
            db.execute("DELETE FROM pending_ir")
        self.assertEqual(prepare_install(self.db,directory,self.capture),first)
        if_occupied = prepare_install(self.db,[*directory,(3,"Later addition")],self.capture)
        self.assertEqual(if_occupied["captureSlot"],4)
        confirm_install(self.db,self.capture,4)
        with closing(sqlite3.connect(self.db)) as db:
            self.assertEqual(db.execute("SELECT slot FROM captures").fetchall(),[(4,)])
            self.assertFalse(db.execute("SELECT slot FROM pending_ir").fetchall())
        self.assertIsNone(self.server.controls.process)

    def test_mixed_storage_and_retries(self):
        def wav(samples):
            return (b"RIFF" + struct.pack("<I", 36+len(samples)) + b"WAVEfmt " +
                    struct.pack("<IHHIIHH",16,3,1,48000,192000,4,32) +
                    b"data"+struct.pack("<I",len(samples))+samples)
        raw = wav(struct.pack("<ff",1.,-.25))
        occupied = [(n,"Owner") for n in range(1,65)]
        cab = prepare_ir_install(self.db,occupied,"V30.wav","cab",raw)
        self.assertEqual(cab["captureSlot"],65)
        self.assertTrue(cab["captureLabel"].startswith("IR Cab V30-"))
        self.assertEqual(bytes.fromhex(cab["sampleBytes"]),raw[44:]+bytes(8184))
        self.assertEqual(prepare_ir_install(self.db,occupied+[(65,cab["captureLabel"])],"V30.wav","cab",raw),cab)
        reverb=prepare_ir_install(self.db,occupied,"Plate.wav","reverb",raw)
        self.assertEqual(reverb["captureSlot"],66)
        self.assertTrue(reverb["captureLabel"].startswith("IR Rev Plate-"))
        with self.assertRaisesRegex(ValueError,"No empty"):
            prepare_ir_install(self.db,[(i,"Owner") for i in range(1,129)],"Plate.wav","reverb",raw)
        for bad in (bytes(8),wav(struct.pack("<fff",.5,-.5,3.)),wav(bytes(2049*4)),wav(struct.pack("<f",float("nan")))):
            with self.assertRaises(ValueError):
                impulse_samples(bad)

        controls = self.server.controls
        controls.process = SimpleNamespace(stdin=io.StringIO())
        controls.state.update(online=True,session="test",rev=3,board={"captureDirectory":occupied})
        headers = {"Content-Type":"application/octet-stream","X-IR-Name":"V30.wav",
                   "X-IR-Kind":"cab","X-Floor-Session":"test","X-Floor-Rev":"3"}
        with self.assertRaises(HTTPError):
            self.request("/live/ir/install",raw,{**headers,"X-Floor-Rev":"2"})
        self.assertFalse(controls.process.stdin.getvalue())
        with self.assertRaises(HTTPError) as blocked:
            self.request("/live/ir/install",raw,headers)
        self.assertEqual(403, blocked.exception.code)
        self.assertFalse(controls.process.stdin.getvalue())
        self.assertIsNone(controls.installing)


if __name__ == "__main__":
    unittest.main()

