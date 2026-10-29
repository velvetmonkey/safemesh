#!/usr/bin/env python3
"""Plant toolchain errors in private copies of the release workflow."""

from pathlib import Path
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parent.parent
CHECK = Path(__file__).with_name('check-release-workflow.py')
SOURCE = (ROOT / '.github/workflows/release.yml').read_text()
VERSION = '          version="$(python3 -c \'import tomllib; print(tomllib.load(open("rust-toolchain.toml", "rb"))["toolchain"]["channel"])\')"\n'
ECHO = '          echo "RUSTUP_TOOLCHAIN=$version" >> "$GITHUB_ENV"\n'
EXPORT = '          export RUSTUP_TOOLCHAIN="$version"\n'
CARGO = '          cargo install wasm-pack --version 0.15.0 --locked\n'


def replace_once(text: str, old: str, new: str) -> str:
    assert text.count(old) == 1, (old, text.count(old))
    return text.replace(old, new, 1)


def run_case(name: str, text: str, should_fail: bool) -> bool:
    with tempfile.TemporaryDirectory() as private:
        path = Path(private) / 'release.yml'
        path.write_text(text)
        result = subprocess.run(
            [sys.executable, str(CHECK), '--file', str(path)],
            text=True, capture_output=True,
        )
    good = result.returncode != 0 if should_fail else result.returncode == 0
    if should_fail:
        good = good and 'release asset tools pinned Rust toolchain' in result.stderr
    print(f'{name} {"PASS" if good else "FAIL"}: exit {result.returncode}')
    return good


def main() -> int:
    cases = [
        ('T1', SOURCE, False),
        ('T2', replace_once(SOURCE, ECHO, '          # ' + ECHO.lstrip()), True),
        ('T3', replace_once(SOURCE, EXPORT, '          # ' + EXPORT.lstrip()), True),
        ('T4', replace_once(SOURCE, VERSION, ''), True),
        ('T5', replace_once(replace_once(SOURCE, VERSION, ''), EXPORT, EXPORT + VERSION), True),
        ('T6', replace_once(replace_once(SOURCE, EXPORT, ''), CARGO, CARGO + EXPORT), True),
        ('T7', replace_once(SOURCE, '      - name: Install asset tools\n', '      - name: Other asset tools\n'), True),
        ('T8', replace_once(SOURCE, ECHO, '          true # ' + ECHO.lstrip()), True),
    ]
    passed = sum(run_case(*case) for case in cases)
    print(f'TESTS 8, PASSED {passed}')
    return 0 if passed == 8 else 1


if __name__ == '__main__':
    sys.exit(main())
