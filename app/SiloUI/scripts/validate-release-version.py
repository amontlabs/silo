"""Fail release builds when the app and package versions disagree."""
import json
from pathlib import Path
import re
import tomllib

root = Path(__file__).resolve().parent.parent
version = json.loads((root / 'package.json').read_text())['version']
if not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', version) or version == '0.0.0':
    raise RuntimeError('A nonzero stable app version is required.')
lock = json.loads((root / 'package-lock.json').read_text())
cargo = tomllib.loads((root / 'src-tauri/Cargo.toml').read_text())
cargo_lock = tomllib.loads((root / 'Cargo.lock').read_text())
versions = [lock['version'], lock['packages']['']['version'], cargo['package']['version']]
versions.extend(p['version'] for p in cargo_lock['package'] if p['name'] == 'silo-ui')
if len(versions) != 4 or any(v != version for v in versions):
    raise RuntimeError('package.json, npm lock, Cargo.toml and Cargo.lock versions must agree.')
print(version)
