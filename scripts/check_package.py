"""Check source hashes, imports and asset paths without opening USB or importing the app."""
import ast
import hashlib
import json
from pathlib import Path
import re
ROOT = Path(__file__).resolve().parents[1]
manifest = json.loads((ROOT / 'SOURCE-MANIFEST.json').read_text())
bad = []
for row in manifest['files']:
    path = ROOT / row['path']
    if hashlib.sha256(path.read_bytes()).hexdigest() != row['sha256']:
        bad.append('changed source: ' + row['path'])
    if path.suffix.lower() in {'.hxf','.hxb','.hlx','.nam','.wav','.exe','.sqlite3','.pcap','.pcapng'}:
        bad.append('excluded artifact: ' + row['path'])
    if path.suffix == '.py':
        ast.parse(path.read_text(encoding='utf-8-sig'), filename=str(path))
    if path.suffix in {'.py','.js','.html','.css','.toml','.rs'}:
        text = path.read_text(encoding='utf-8-sig')
        # Upstream home.rs contains its author's public synthetic Windows path tests.
        if re.search(r'[CE]:[\\/](?:Users[\\/]nicko|Dev[\\/]HelixNam)', text, re.I):
            bad.append('machine-specific path: ' + row['path'])
for name in ('live-controls.html',):
    text = (ROOT/'design'/name).read_text()
    for rel in re.findall(r'(?:src|href)="/?(design/[^"?#]+)', text):
        if not (ROOT/rel).is_file():
            bad.append('missing asset: '+rel)
if bad:
    raise SystemExit('\n'.join(bad))
print(f"PASS: {len(manifest['files'])} source hashes, syntax and package exclusions checked.")
