"""Offline tests for the live capture shelf and the bridge's trusted write input."""
import json
from pathlib import Path
import struct
import tempfile
import unittest

from capture_library import Library
from floor_nam_navigation import LEGACY_LABELS, capture_payload, classify_capture, installed_matches, name_live_blocks, nam_rows, prepare_selection, source_engines


class CaptureShelfTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=Path(__file__).resolve().parent / "build")
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / "captures.sqlite3"
        self.key = "0abcdef1" + "a" * 56
        self.label = "My-amp-0abcdef1-Lite"
        vals = [0.5, -0.5, 3, 1, 1871 / 1024] + [0] * 2043
        self.samples = struct.pack("<2048f", *vals)
        wav = (b"RIFF" + struct.pack("<I", 8228) + b"WAVEfmt " +
               struct.pack("<IHHIIHH", 16,3,1,48000,192000,4,32) +
               b"data" + struct.pack("<I",8192) + self.samples)
        self.lib = Library(self.path)
        self.addCleanup(self.lib.db.close)
        self.lib.db.execute("INSERT INTO captures (id,name,original,wav,compatibility,override,slot) "
                            "VALUES(?, ?, ?, ?, 'A2 Lite', 'Amp head', 11)",
                            (self.key, "My amp", b'{"architecture":"WaveNet"}', wav))
        self.lib.db.commit()

    def test_directory_overrides_stale_local_slot_and_absent_metadata(self):
        directory = [(11,"Other"),(13,self.label)]
        row, = nam_rows(self.path, directory)
        self.assertEqual((row["kind"],row["instrument"],row["installed"]),("Amp head","Unknown",True))
        self.assertEqual(prepare_selection(self.path,directory,self.key),
                         {"captureCopies":[(13,self.label)], "sampleBytes":self.samples.hex()})
        self.assertFalse(nam_rows(self.path,[])[0]["installed"])
        with self.assertRaises(ValueError):
            prepare_selection(self.path,[(11,"Other")],self.key)

    def test_duplicate_installed_copies_have_one_identity_and_prefer_active_block(self):
        directory = [(20,self.label),(11,self.label)]
        row, = nam_rows(self.path,directory)
        self.assertTrue(row["installed"])
        for slot in (11,20):
            board = {"blocks":[{"model":380,"params":[{"value":float(slot)}]}],"captureDirectory":directory}
            name_live_blocks(self.path,board)
            self.assertEqual(board["blocks"][0]["namId"],self.key)
        self.assertEqual(prepare_selection(self.path,directory,self.key)["captureCopies"],[(11,self.label),(20,self.label)])
        self.assertEqual(prepare_selection(self.path,directory,self.key,20)["captureCopies"],directory)

    def test_ambiguous_hash_prefixes_do_not_select(self):
        self.assertFalse(installed_matches([(3,self.label)], [self.key, self.key[:8]+"e"*56]))

    def test_name_enrichment_preserves_verifiable_identity(self):
        board={"blocks":[{"model":380,"params":[{"value":3.0}]}],"captureDirectory":[[3,self.label]]}
        name_live_blocks(self.path,board)
        self.assertEqual(board["blocks"][0]["namId"],self.key)
        self.assertEqual(board["blocks"][0]["name"],"My amp")
        metadata={"metadata":{"instrument":"bass", "tone_type":"rock"}}
        self.lib.db.execute("UPDATE captures SET original=?", (json.dumps(metadata).encode(),))
        self.lib.db.commit()
        self.assertEqual(nam_rows(self.path,[])[0]["instrument"],"Bass")

    def test_invalid_payload_is_rejected(self):
        with self.lib.db:
            self.lib.db.execute("UPDATE captures SET wav=?", (b"RIFF",))
        with self.assertRaises(ValueError):
            capture_payload(self.path,self.key)

    def test_local_tags_survive_legacy_classify_and_reset_to_source(self):
        with self.lib.db:
            self.lib.db.execute("UPDATE captures SET original=?, file_gear='pedal'", (
                b'{"metadata":{"instrument":"electric guitar"}}',))
        classify_capture(self.path,self.key,kind="Cab",instrument="Bass")
        tagged, = nam_rows(self.path,[])
        self.assertEqual((tagged["overrideKind"],tagged["overrideInstrument"]),("Cab","Bass"))
        self.lib.classify(self.key,"Amp + cab")
        tagged, = nam_rows(self.path,[])
        self.assertEqual((tagged["kind"],tagged["instrument"]),("Amp + cab","Bass"))
        with self.assertRaises(ValueError):
            classify_capture(self.path,self.key,kind="Cab",instrument="Banjo")
        self.assertEqual(nam_rows(self.path,[])[0]["kind"],"Amp + cab")
        classify_capture(self.path,self.key,kind=None,instrument=None)
        restored, = nam_rows(self.path,[])
        self.assertEqual((restored["kind"],restored["instrument"]),("Pedal","Guitar"))
        self.assertIsNone(restored["overrideInstrument"])

    def test_full_and_lite_are_classified_by_the_verified_layouts(self):
        model=json.loads((Path(__file__).resolve().parent / "tests/synthetic-a2.json").read_bytes())
        self.assertEqual(source_engines(model),("A2 Lite","A2 Full"))
        full=next(s["model"] for s in model["config"]["submodels"] if len(s["model"]["weights"])==12146)
        self.assertEqual(source_engines(full),("A2 Full",))
        self.assertEqual(source_engines({**full,"sample_rate":44100}),())
        self.assertEqual(source_engines({**full,"weights":full["weights"][:-1]}),())
        full["config"]["layers"][0]["dilations"][0]+=1
        self.assertEqual(source_engines(full),())

    def test_existing_plexi_keeps_its_identity_and_disputed_gear_labels(self):
        key = LEGACY_LABELS["Plexi-815147-Lite"]
        with self.lib.db:
            self.lib.db.execute("UPDATE captures SET id=?, file_gear='amp', override=NULL", (key,))
            self.lib.db.execute("INSERT INTO sources VALUES(?,?,?)", (key,"Legacy rig","amp-cab"))
        directory = [(10, "Plexi-815147-Lite")]
        row, = nam_rows(self.path, directory)
        self.assertEqual((row["name"],row["kind"],row["sourceKind"]), ("Plexi","Unknown",None))
        self.assertEqual((row["fileGear"],row["listingGear"]), ("amp","amp-cab"))
        self.assertTrue(row["installed"])
        self.lib.classify(key, "Amp + cab")
        self.assertEqual(nam_rows(self.path, directory)[0]["kind"], "Amp + cab")


if __name__ == "__main__":
    unittest.main()
