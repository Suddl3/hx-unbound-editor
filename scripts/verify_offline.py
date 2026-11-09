"""Bounded offline checks. No browser, firmware operation or hardware connection."""
import json
from pathlib import Path
import subprocess
import sys
ROOT = Path(__file__).resolve().parents[1]
(ROOT/'build').mkdir(exist_ok=True)
commands = [
    [sys.executable, '-B', 'scripts/check_package.py'],
    [sys.executable, '-B', '-m', 'unittest', 'verify_configured_favorites', 'verify_nam_navigation', 'verify_nam_shelf_workflow', 'verify_preview_policy'],
]
commands += [['node','--check',str(p.relative_to(ROOT))] for p in sorted((ROOT/'design').glob('*.js'))]
for command in commands:
    result = subprocess.run(command, cwd=ROOT, timeout=60, capture_output=True, text=True)
    if result.returncode:
        print(result.stdout + result.stderr)
        raise SystemExit(result.returncode)
    if result.stdout.strip() or result.stderr.strip():
        print(result.stdout + result.stderr)
print(json.dumps({'status':'passed','commands':len(commands),'hardwareAccess':False}))
