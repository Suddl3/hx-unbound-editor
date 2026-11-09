"""Read saved TONE3000 schema-1 rigs and embedded active NAM models."""
import json
import struct


def integer(raw):
    if len(raw) != 5 or raw[0] != 1:
        raise ValueError("Expected a preset integer")
    return struct.unpack_from("<i", raw, 1)[0]


def read_preset(path, *, data=None):
    if data is None:
        data = path.read_bytes()
    if len(data) < 8 or data[:4] not in (b"T3KH", b"T3KB"):
        raise ValueError("Unrecognized TONE3000 preset")
    has_header = data[:4] == b"T3KH"
    pos = 8 if has_header else 4

    def take(count):
        nonlocal pos
        if count < 0 or count > len(data) - pos:
            raise ValueError("Truncated preset field")
        pos += count
        return data[pos-count:pos]

    def count():
        width = take(1)[0]
        if width > 4:
            raise ValueError("Invalid preset length")
        return int.from_bytes(take(width), "little")

    def name():
        end = data.index(0, pos)
        return take(end - pos + 1)[:-1].decode("utf-8")

    def tree(depth=0):
        if depth > 64:
            raise ValueError("Unexpected preset nesting")
        node = name()
        props = {}
        for _ in range(count()):
            key = name()
            props[key] = take(count())
        children = [tree(depth + 1) for _ in range(count())]
        return node, props, children

    if has_header:
        header, _, _ = tree()
        if header != "T3KPresetHeader" or pos != 8 + int.from_bytes(data[4:8], "little"):
            raise ValueError("Unexpected preset header")
    root, props, children = tree()
    if root != "T3KPreset" or integer(props["schemaVersion"]) != 1 or pos != len(data):
        raise ValueError("Expected one schema-1 preset")
    return props, children


def active_captures(path, *, parsed=None):
    """Return {model_id: (display_name, cached_bytes_or_None)} without changing the file."""
    _, children = parsed if parsed is not None else read_preset(path)
    captures = {}
    for node, _, branches in children:
        if node != "ChainSnapshot":
            continue
        for branch, _, blocks in branches:
            if branch not in ("ChainBlocks", "RightChainBlocks"):
                continue
            for block, fields, sections in blocks:
                if block != "ChainBlock" or fields.get("type") != b"\x05nam\0":
                    continue
                model_id = integer(fields["activeModelId"])
                display = str(model_id)
                tone = fields.get("toneJson", b"")
                if tone.startswith(b"\x05") and tone.endswith(b"\0"):
                    models = json.loads(tone[1:-1]).get("models", [])
                    display = next((m["name"] for m in models if m["id"] == model_id), display)
                cached = captures.get(model_id, (display, None))[1]
                for section, _, entries in sections:
                    if section != "ModelCache":
                        continue
                    for entry, item, _ in entries:
                        if entry == "CachedModel" and integer(item["modelId"]) == model_id:
                            raw = item["data"]
                            if not raw.startswith(b"\x08") or len(raw) == 1:
                                raise ValueError(f"Invalid model cache for {model_id}")
                            if cached is not None and cached != raw[1:]:
                                raise ValueError(f"Conflicting cached versions of {model_id}")
                            cached = raw[1:]
                captures[model_id] = (display, cached)
    return captures
