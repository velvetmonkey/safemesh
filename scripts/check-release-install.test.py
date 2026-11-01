#!/usr/bin/env python3
"""Planted input checks for the release artifact installer."""
import hashlib
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / 'scripts/check-release-install.sh'
NAMES = ('safemesh-crdt-0.2.2.crate',
         'safemesh_python-0.2.2-py3-none-any.whl',
         'safemesh-wasm-0.2.2.tgz')


def plant(folder, names=NAMES, corrupt=None):
    lines = []
    for name in names:
        data = name.encode()
        (folder / name).write_bytes(data)
        lines.append(f'{hashlib.sha256(data).hexdigest()}  {name}\n')
    (folder / 'SHA256SUMS').write_text(''.join(lines))
    if corrupt:
        (folder / corrupt).write_bytes(b'changed')


def check(name, names=NAMES, corrupt=None, expected=''):
    with tempfile.TemporaryDirectory(prefix='release-install-test-') as raw:
        folder = Path(raw)
        plant(folder, names, corrupt)
        result = subprocess.run(['bash', str(SCRIPT), str(folder)],
                                cwd=ROOT, text=True, capture_output=True)
        output = result.stdout + result.stderr
        assert result.returncode != 0, f'{name}: accepted planted input: {output}'
        assert expected.lower() in output.lower(), f'{name}: cause absent: {output}'
        print(f'PLANT {name} red')


check('T3 missing crate', NAMES[1:], expected='3 packages')
check('T4 sum mismatch', corrupt=NAMES[2], expected='FAILED')
check('mixed versions', (NAMES[0], NAMES[1], 'safemesh-wasm-0.2.1.tgz'), expected='version')
check('duplicate wheel', (*NAMES, 'safemesh_python-0.2.2-py2-none-any.whl'), expected='3 packages')
