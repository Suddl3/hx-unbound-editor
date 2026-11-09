"""Single-block edits for a simple spare NAM preset on Floor firmware 3.80."""
import hashlib
import json
import re
import subprocess


NAM = "model 380"
BOOST = "model 388"  # HX Edit 3.80 Helix.sym: HD2_DistKinkyBoostMono
REFERENCE_PRESETS = set()  # Personal development presets are not part of the public preview.


def edit_revision(view):
    blocks = view["blocks"]
    positions = [b["position"] for b in blocks]
    if (view["firmware"] != "3.80" or (view["label"], view["name"]) in REFERENCE_PRESETS
            or sum(b["model"] == NAM for b in blocks) != 1
            or len(blocks) not in (1, 2) or any(b["model"] not in (NAM, BOOST) for b in blocks)
            or len(set(positions)) != len(positions) or any(not 2 <= p <= 9 for p in positions)):
        return None
    lanes = view["lanes"]
    if len(lanes) != 2 or not re.fullmatch(r"20 Input \[0\]\s+->\s+29 Output \[\d+\]", lanes[1]):
        return None
    parts = re.split(r"\s+->\s+", lanes[0])
    cells = [f"{b['position']-1} {b['model'].split()[1]}" + ("" if b["enabled"] else " (off)")
             for b in sorted(blocks, key=lambda b:b["position"])]
    if (not re.fullmatch(r"0 Input \[[1-9]\d*\]", parts[0]) or not re.fullmatch(r"9 Output \[\d+\]", parts[-1])
            or parts[1:-1] != cells):
        return None
    identity = [view["label"], view["name"], lanes, blocks]
    return hashlib.sha256(json.dumps(identity,sort_keys=True).encode()).hexdigest()


def boost_identity(view):
    """Keep session ownership across boost bypass changes only."""
    if not edit_revision(view) or not any(b["model"] == BOOST for b in view["blocks"]):
        return None
    identity = [view["label"], view["name"], view["firmware"],
                re.sub(r"(\b\d+ 388) \(off\)", r"\1", view["lanes"][0]), view["lanes"][1],
                [{**b, "enabled":True} if b["model"] == BOOST else b for b in view["blocks"]]]
    return hashlib.sha256(json.dumps(identity,sort_keys=True).encode()).hexdigest()


def edit_boost(action, position, revision, read, command, *, before=None, write_and_read=None, report=None):
    if before is None:
        before = read()
    if not revision or edit_revision(before) != revision:
        raise ValueError("Preset or path changed. Select the slot again.")
    slot = position + 1  # CLI counts Input as position 1.
    original = {b["position"]:b for b in before["blocks"]}
    boost = next((b for b in original.values() if b["model"] == BOOST), None)
    if not 1 <= position <= 8:
        raise ValueError("Select a position on path 1A.")
    if action == "move":
        if not boost or slot in original:
            raise ValueError("Select an empty destination for the boost.")
        params = boost["params"]
        if (not boost["enabled"] or set(params) != {"param0","param1","param2"} or
                abs(params["param0"] - 0.55) > 0.0001 or params["param1"] != 0 or params["param2"] != 0):
            raise ValueError("Repositioning requires an enabled Kinky Boost at its factory settings.")
        # Recreate only the stock block. The CLI move command rewrites the entire preset.
        if report:
            report(f"Removing Kinky Boost from position {boost['position']-1} and checking the path")
        empty = edit_boost("remove", boost["position"]-1, revision, read, command,
                           before=before, write_and_read=write_and_read)
        if report:
            report(f"Removal confirmed. Adding Kinky Boost at position {position} and checking the path")
        try:
            moved = edit_boost("add", position, edit_revision(empty), read, command,
                               before=empty, write_and_read=write_and_read)
        except (ValueError, OSError, subprocess.SubprocessError) as exc:
            raise ValueError("Original boost removed. Destination unconfirmed. Read the path before another edit.") from exc
        recreated = next(b for b in moved["blocks"] if b["position"] == slot)
        if recreated != {**boost, "position": slot}:
            raise ValueError("Boost was re-created with different settings. Check the Floor before comparing.")
        return moved
    if action == "add":
        if slot in original or boost is not None:
            raise ValueError("Choose an empty position on a path with just one NAM amp.")
        instruction = f"model {slot} 388"
    else:
        if not boost or boost["position"] != slot:
            raise ValueError("The selected position does not hold Kinky Boost.")
        instruction = f"clear {slot}" if action == "remove" else f"enable {slot} {action}"
    if action not in ("add","remove","on","off") or not 1 <= position <= 8:
        raise ValueError("Unknown boost edit.")
    if action in ("on","off") and boost["enabled"] == (action == "on"):
        return before
    if write_and_read:
        after = write_and_read(instruction + "\n")
    else:
        command(instruction + "\n")
        after = read()
    current = {b["position"]:b for b in after["blocks"]}
    target = current.pop(slot,None)
    original.pop(slot,None)
    if (after["label"] != before["label"] or after["name"] != before["name"] or not edit_revision(after)
            or original != current or (action=="remove" and target is not None)
            or (action!="remove" and (not target or target["model"]!=BOOST))
            or (action in ("on","off") and target["enabled"] != (action=="on"))):
        raise ValueError("The edit did not match the Floor readback. Re-read the path before continuing.")
    return after
