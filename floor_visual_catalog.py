"""Build the model browser from the HX Edit installation on this computer."""
import hashlib
import io
import json
import os
import re
from functools import lru_cache
from pathlib import Path


def installed_resources():
    return Path(os.environ.get("ProgramFiles(x86)", "C:/Program Files (x86)")) / "Line6" / "HX Edit" / "res"


def _read_json(path):
    with path.open(encoding="utf-8-sig") as stream:
        return json.load(stream)


class ArtIndex:
    def __init__(self, root):
        self.root = root
        self.files = {}
        self.urls = {}

    def icon(self, directory, name):
        if not name or Path(name).name != name or not name.lower().endswith(".png"):
            return None
        key = (directory, name)
        if key in self.urls:
            return self.urls[key]
        path = self.root / directory / name
        if not path.is_file() or not path.resolve().is_relative_to((self.root / directory).resolve()):
            self.urls[key] = None
            return None
        stamp = path.stat()
        token = hashlib.sha256(f"{directory}/{name}/{stamp.st_size}/{stamp.st_mtime_ns}".encode()).hexdigest()[:24]
        self.files[token] = path
        self.urls[key] = f"/live/art/{token}.png"
        return self.urls[key]

    def read(self, token):
        path = self.files[token]
        strip = re.search(r"_%(\d+)\.png$", path.name, re.I)
        if not strip:
            return path.read_bytes()
        # Direction and endpoint icons are vertical sprite strips.
        from PIL import Image
        with Image.open(path) as image:
            frames = int(strip.group(1))
            if frames <= 0 or image.height % frames:
                raise ValueError("Invalid HX Edit icon strip")
            with io.BytesIO() as output:
                image.crop((0, 0, image.width, image.height // frames)).save(output, "PNG")
                return output.getvalue()


def _shelves(category):
    if category.get("models"):
        yield "", category["models"]
    for shelf in category.get("subcategories", []):
        yield shelf["name"], shelf["models"]


def _wire_id(symbol_id, shelf, numbers):
    if shelf in ("Mono", "Stereo"):
        variant = numbers.get(symbol_id + shelf)
        if variant is not None:
            return variant
    number = numbers.get(symbol_id)
    if number is None and shelf == "Legacy":
        # Some legacy reverbs have a single wire symbol with a Stereo suffix.
        variants = [numbers[symbol_id + suffix] for suffix in ("Mono", "Stereo")
                    if symbol_id + suffix in numbers]
        if len(variants) == 1:
            return variants[0]
    return number


@lru_cache(maxsize=1)
def visual_catalog(resources, placements):
    """Return public metadata and the private index of allowed local assets."""
    required = ('HX_ModelCatalog.json', 'Helix.sym', 'amp.models')
    if not all((resources / name).is_file() for name in required):
        return json.dumps({'categories': [], 'models': [],
            'notice': 'Install HX Edit for stock model names and locally supplied artwork. Local NAM browsing is available.'}).encode(), ArtIndex(resources)
    catalog = _read_json(resources / "HX_ModelCatalog.json")["categories"]
    symbols = _read_json(resources / "Helix.sym")
    numbers = {s["symbol"]: i for i, s in enumerate(symbols)}
    permitted = {m["model"]: m for m in json.loads(placements)}
    amps = {m["symbolicID"] for m in _read_json(resources / "amp.models") if m.get("cablink")}
    art = ArtIndex(resources)
    categories, models = [], []
    for category in catalog:
        category_name = category["name"]
        category_icon = art.icon("icons_category", category.get("image"))
        # The published catalog contains the device families side by side.
        # The connected-device category provides its imagery on its model rows.
        if category_name == "Connected Devices" and not category_icon:
            category_icon = art.icon("icons_models", "icon-variax.png")
        color = "#" + category.get("color", "0xffffff").removeprefix("0x")
        shelves = list(_shelves(category))
        cat = {"name": category_name, "icon": category_icon, "color": color,
               "shelves": [s for s, _ in shelves if s]}
        # Mono and Stereo (also Single/Dual Legacy) share their artwork.
        # HX Edit's use_subcategory rows refer to a full entry by symbol ID.
        details = {m["id"]: m for _, entries in shelves for m in entries if m.get("name")}
        rows = []
        for shelf, entries in shelves:
            for entry in entries:
                symbol_id = entry["id"]
                if symbol_id.startswith(("HelixFx_", "HelixStomp_")):
                    continue
                detail = details.get(symbol_id, {})
                number = _wire_id(symbol_id, shelf, numbers)
                choice = permitted.get(number, {})
                name = entry.get("name") or detail.get("name") or choice.get("name") or symbol_id
                image = entry.get("image") or detail.get("image")
                icon = art.icon("icons_models", image)
                placeable = (choice.get("category") == category_name and
                             (category_name != "Cab" or shelf in ("Single", "Single Legacy")))
                rows.append({
                    "key": f"{category['id']}/{shelf}/{symbol_id}", "name": name, "model": number,
                    "category": category_name, "shelf": shelf, "variant": choice.get("variant", ""),
                    "instrument": shelf.lower() if shelf in ("Guitar", "Bass") else None,
                    "icon": icon or category_icon, "iconSource": "model" if icon else "category",
                    "placeable": placeable, "symbol": symbol_id,
                })
        if category_name == "Amp":
            # HX Edit constructs Amp+Cab from the amp's cablink and lists it ahead of Amp.
            categories.append({**cat, "name": "Amp+Cab", "icon": art.icon("icons_category", "FX_HX_Category_Amp+Cab.png")})
            for row in rows:
                if row["symbol"] in amps:
                    cab = permitted.get(row["model"], {}).get("defaultCab")
                    models.append({**row, "key": "pair/" + row["key"], "category": "Amp+Cab",
                                   "pairedModel": cab, "placeable": cab is not None})
        categories.append(cat)
        models.extend(rows)
    payload = json.dumps({"categories": categories, "models": models}, ensure_ascii=False,
                         separators=(",", ":")).encode("utf-8")
    return payload, art
