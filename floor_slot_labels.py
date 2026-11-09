"""Compact display labels shared by the Floor installers and WAV export."""
import re
import unicodedata


def floor_label(prefix, name, identity):
    ascii_name = unicodedata.normalize("NFKD", name).encode("ascii", "ignore").decode()
    stem = re.sub(r"[^a-zA-Z0-9 -]+", " ", ascii_name)
    stem = " ".join(stem.split()).strip(" -") or "Untitled"
    # The USB descriptor allows 31 bytes. Retain the hash for identity checks.
    stem = stem[:31 - len(prefix) - 10].rstrip(" -") or "Untitled"
    return f"{prefix} {stem}-{identity[:8]}"
