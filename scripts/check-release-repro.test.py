#!/usr/bin/env python3
"""Plant release asset differences in private copies of one real build."""

import argparse
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import zipfile


CHECK = Path(__file__).with_name("check-release-repro.py")


def run(check: Path, a: Path, b: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run([sys.executable, str(check), "--compare-only", str(a), str(b)],
                          text=True, capture_output=True)


def rewrite_wheel(path: Path) -> str:
    with zipfile.ZipFile(path) as source:
        members = [(info, source.read(info)) for info in source.infolist()]
    name = next(info.filename for info, _ in members if info.filename.endswith("/__init__.py"))
    with zipfile.ZipFile(path, "w") as output:
        for info, data in members:
            output.writestr(info, data + b"changed" if info.filename == name else data)
    return name


def rewrite_tgz(path: Path) -> str:
    with tarfile.open(path, "r:gz") as source:
        members = [(info, source.extractfile(info).read() if info.isfile() else None)
                   for info in source.getmembers()]
    name = next(info.name for info, data in members if data is not None and info.name.endswith("package.json"))
    with tarfile.open(path, "w:gz") as output:
        for info, data in members:
            if info.name == name:
                data += b"changed"
                info.size = len(data)
            output.addfile(info, __import__("io").BytesIO(data) if data is not None else None)
    return name


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--assets", type=Path)
    parser.add_argument("--check", type=Path, default=CHECK)
    args = parser.parse_args()
    assets = args.assets or Path(__import__("os").environ["RUNNER_TEMP"]) / "release-assets"
    if not assets.is_dir() or not (assets / "SHA256SUMS").is_file():
        parser.error(f"real assets folder is absent: {assets}")
    private_root = Path(__import__("os").environ.get("RUNNER_TEMP") or
                        __import__("os").environ.get("TMPDIR") or tempfile.gettempdir())
    failures = []
    with tempfile.TemporaryDirectory(prefix="release-repro-test-", dir=private_root) as private:
        root = Path(private)
        a = root / "a"
        shutil.copytree(assets, a)
        crate = next(a.glob("*.crate")).name
        wheel = next(a.glob("*.whl")).name
        tgz = next(a.glob("*.tgz")).name
        cases = []
        for index in range(1, 8):
            b = root / f"b{index}"
            if index != 7:
                shutil.copytree(a, b)
            else:
                b.mkdir()
            needle = "RELEASE REPRO FILES"
            if index == 2:
                path = b / crate
                data = path.read_bytes()
                path.write_bytes(bytes([data[0] ^ 1]) + data[1:])
                needle = crate
            elif index == 3:
                needle = rewrite_wheel(b / wheel)
            elif index == 4:
                needle = rewrite_tgz(b / tgz)
            elif index == 5:
                (b / "unexpected.file").write_bytes(b"extra")
                needle = "unexpected.file"
            elif index == 6:
                (b / "SHA256SUMS").unlink()
                needle = "SHA256SUMS"
            elif index == 7:
                needle = "empty or absent folder"
            cases.append((f"T{index}", b, index != 1, needle))
        for label, b, red, needle in cases:
            result = run(args.check, a, b)
            ok = (result.returncode != 0 if red else result.returncode == 0) and needle in result.stdout
            print(f"{label} {'PASS' if ok else 'FAIL'}: exit {result.returncode}; {needle}")
            if not ok:
                failures.append(label)
                print(result.stdout, end="")
                print(result.stderr, end="", file=sys.stderr)
        stub = root / "build-exit-7"
        stub.write_text("#!/bin/sh\nexit 7\n")
        stub.chmod(0o755)
        result = subprocess.run([sys.executable, str(args.check), str(a), "--build-command", str(stub)],
                                text=True, capture_output=True)
        ok = result.returncode != 0 and "build B exit 7" in result.stdout
        print(f"T8 {'PASS' if ok else 'FAIL'}: exit {result.returncode}; build B exit 7")
        if not ok:
            failures.append("T8")
            print(result.stdout, end="")
            print(result.stderr, end="", file=sys.stderr)
    return int(bool(failures))


if __name__ == "__main__":
    sys.exit(main())
