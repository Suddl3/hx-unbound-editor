"""Initialize empty local data directories. Does not contact the device."""
from pathlib import Path
import sys
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from capture_library import Library

for folder in ('build/capture-browser', 'build/floor-session'):
    (ROOT / folder).mkdir(parents=True, exist_ok=True)
library = Library(ROOT / 'build/capture-browser/library.sqlite3')
library.db.close()
print('Local empty library is ready. No capture slots or device state assumed.')
