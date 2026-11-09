"""Convert supported A2 Lite captures using the pinned GPL converter."""
import math
from pathlib import Path
import struct
import subprocess
import sys
from tempfile import TemporaryDirectory
ROOT = Path(__file__).resolve().parent
TOOLS = ROOT / "vendor/nam/tools"

def encode_capture(capture, model):
    scratch = ROOT / "build"
    scratch.mkdir(exist_ok=True)
    with TemporaryDirectory(dir=scratch) as tmp:
        source, wav = Path(tmp) / "amp.nam", Path(tmp) / "amp.wav"
        source.write_bytes(capture)
        result = subprocess.run([sys.executable, "-X", "utf8", "-B", str(TOOLS / "nam2ir.py"),
                                 str(source), "-o", str(wav)], timeout=90, capture_output=True,
                                encoding="utf-8", creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        if result.returncode:
            raise ValueError(result.stderr.strip() or "Floor conversion failed")
        raw = wav.read_bytes()
    if (raw[:4] != b"RIFF" or raw[8:12] != b"WAVE" or raw[36:40] != b"data"
            or len(raw) != 8236 or struct.unpack_from("<HHI", raw, 20) != (3, 1, 48000)
            or struct.unpack_from("<I", raw, 40)[0] != 8192):
        raise ValueError("Unexpected converter WAV format or length")
    samples = struct.unpack_from("<2048f", raw, 44)
    if samples[:3] != (0.5, -0.5, 3.0) or samples[4] * 1024 != 1871:
        raise ValueError("Invalid NAM header")
    sys.path.insert(0, str(TOOLS / "sharc"))
    import nam_lite
    expected = nam_lite.stream(model["weights"])
    decoded = [x * samples[3] for x in samples[8:8+len(expected)]]
    fmt = "<1943f"  # 1,871 reordered coefficients plus the initial silence state.
    if (struct.pack(fmt, *expected) != struct.pack(fmt, *decoded)
            or any(samples[8+len(expected):]) or not all(map(math.isfinite, samples))):
        raise ValueError("Encoded capture differs from the model stream")
    return raw

