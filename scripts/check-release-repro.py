#!/usr/bin/env python3
"""Compare release assets from two checkouts of the same commit."""

import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import zipfile


ROOT = Path(__file__).resolve().parent.parent


def digest(path: Path | None) -> str:
    if path is None:
        return "MISSING"
    return hashlib.sha256(path.read_bytes()).hexdigest()


def archive_members(path: Path) -> dict[str, tuple[bytes, object]] | None:
    if path.suffix == ".whl":
        with zipfile.ZipFile(path) as archive:
            return {
                info.filename: (archive.read(info), (info.date_time, info.external_attr))
                for info in archive.infolist()
            }
    if path.name.endswith((".crate", ".tgz", ".tar.gz")):
        with tarfile.open(path, "r:gz") as archive:
            return {
                info.name: (
                    archive.extractfile(info).read() if info.isfile() else b"",
                    (info.type, info.mode, info.uid, info.gid, info.mtime, info.linkname),
                )
                for info in archive.getmembers()
            }
    return None


def first_member(a: Path, b: Path) -> str:
    try:
        left, right = archive_members(a), archive_members(b)
        if left is None or right is None:
            return ""
        for name in sorted(left.keys() | right.keys()):
            if left.get(name) != right.get(name):
                return f" member={name}"
        return " member=<archive header>"
    except (OSError, ValueError, tarfile.TarError, zipfile.BadZipFile, KeyError):
        return " member=<unreadable archive>"


def compare(a: Path, b: Path) -> int:
    left = {p.relative_to(a).as_posix(): p for p in a.rglob("*") if p.is_file()} if a.is_dir() else {}
    right = {p.relative_to(b).as_posix(): p for p in b.rglob("*") if p.is_file()} if b.is_dir() else {}
    names = sorted(left.keys() | right.keys() | {"SHA256SUMS"})
    different = 0
    if not left or not right:
        print(f"RELEASE REPRO ERROR empty or absent folder: A={a} B={b}")
        different += 1
    for name in names:
        pa, pb = left.get(name), right.get(name)
        if pa is not None and pb is not None and pa.read_bytes() == pb.read_bytes():
            continue
        extra = first_member(pa, pb) if pa and pb else ""
        print(f"RELEASE REPRO DIFF {name} A={digest(pa)} B={digest(pb)}{extra}")
        different += 1
    print(f"RELEASE REPRO FILES {len(names)}, DIFFER {different}")
    return int(different != 0)


def build_again(a: Path, build_command: str) -> int:
    runner_temp = Path(os.environ.get("RUNNER_TEMP", tempfile.gettempdir())).resolve()
    runner_temp.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="safemesh-release-repro-", dir=runner_temp) as private:
        private_root = Path(private)
        checkout = private_root / "second-checkout-at-a-different-absolute-path"
        while abs(len(str(checkout)) - len(str(ROOT))) < 5:
            checkout = checkout.parent / (checkout.name + "-longer")
        output = private_root / "build-b"
        head = subprocess.run(["git", "-C", str(ROOT), "rev-parse", "HEAD"],
                              capture_output=True, text=True, check=True).stdout.strip()
        cloned = subprocess.run(["git", "clone", "--no-hardlinks", "--quiet", str(ROOT), str(checkout)])
        if cloned.returncode:
            print(f"RELEASE REPRO ERROR clone exit {cloned.returncode}")
            print("RELEASE REPRO FILES 0, DIFFER 1")
            return 1
        checked = subprocess.run(["git", "-C", str(checkout), "checkout", "--quiet", "--detach", head])
        if checked.returncode:
            print(f"RELEASE REPRO ERROR checkout exit {checked.returncode}")
            print("RELEASE REPRO FILES 0, DIFFER 1")
            return 1
        env = os.environ.copy()
        env.pop("CARGO_TARGET_DIR", None)
        env["TMPDIR"] = str(private_root)
        print(f"RELEASE REPRO CHECKOUT A={ROOT} B={checkout}", flush=True)
        built = subprocess.run([build_command, str(checkout / "scripts/build-release-assets.sh"), str(output)], env=env)
        if built.returncode:
            print(f"RELEASE REPRO ERROR build B exit {built.returncode}")
            print("RELEASE REPRO FILES 0, DIFFER 1")
            return 1
        return compare(a, output)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("assets", nargs="?", type=Path)
    parser.add_argument("--compare-only", nargs=2, type=Path, metavar=("A", "B"))
    parser.add_argument("--build-command", default="bash", help="test stub command")
    args = parser.parse_args()
    if args.compare_only:
        return compare(*args.compare_only)
    if args.assets is None:
        parser.error("assets folder is required")
    return build_again(args.assets, args.build_command)


if __name__ == "__main__":
    sys.exit(main())
