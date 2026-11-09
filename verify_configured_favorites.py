import json
import sqlite3
import threading
import tempfile
import unittest
from pathlib import Path
from contextlib import closing
import floor_configured_blocks as cfg
from floor_control_session import ControlSession
from unittest.mock import Mock, patch


class Favorites(unittest.TestCase):
    def test_board_echo_preserves_local_capture_outcome_once(self):
        with tempfile.TemporaryDirectory(dir=Path(__file__).resolve().parent/"build") as temp:
            for payload, expected in [("01020304", "Saved configured favorite"), ("zz", "Could not save configured favorite")]:
                with self.subTest(payload=payload):
                    usb_lock = threading.Lock()
                    usb_lock.acquire()
                    session = ControlSession(Path(temp), usb_lock)
                    helper_message = "Read configured block from Floor"
                    states = []
                    def events():
                        yield json.dumps({"configuredBlock":{"name":"Boost","model":388,"hex":payload},"message":helper_message})
                        yield json.dumps({"board":{"blocks":[]},"message":helper_message,"pending":False})
                        states.append(session.snapshot())
                        yield json.dumps({"board":{"blocks":[]},"message":"Different edit"})
                        states.append(session.snapshot())
                    proc = Mock(stdout=events())
                    proc.wait.return_value = 0
                    proc.poll.return_value = 0
                    with patch("floor_control_session.name_live_blocks"):
                        session._read(proc, None)
                    self.assertTrue(states[0]["message"].startswith(expected), states[0]["message"])
                    self.assertFalse(states[0]["pending"])
                    self.assertEqual(states[1]["message"], "Different edit")

    def test_local_identity_and_authoritative_payload(self):
        with tempfile.TemporaryDirectory(dir=Path(__file__).resolve().parent/"build") as temp:
            db=Path(temp)/"library.sqlite3"
            self.assertEqual(cfg.rows(db),[])
            raw=b"stored opaque slot"
            for _ in range(2):
                event={"configuredBlock":{"name":"My boost","model":388,"hex":raw.hex()}}
                cfg.receive(db,event)
                self.assertNotIn("configuredBlock",event)
            row,=cfg.rows(db)
            result=cfg.trusted_apply(db,{"favorite":row["id"],"slot":2,"rev":8,"model":999,"blockHex":"ff"})
            self.assertEqual((result["blockHex"],result["model"],result["slot"]),(raw.hex(),388,2))
            with closing(sqlite3.connect(db)) as conn, conn:
                conn.execute("UPDATE configured_blocks SET raw=?",(b"other bytes",))
            with self.assertRaises(ValueError):
                cfg.trusted_apply(db,{"favorite":row["id"],"slot":2,"rev":9})


if __name__=="__main__":
    unittest.main()
