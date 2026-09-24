#!/usr/bin/env python3
"""Generate or execute the pinned consumer recipe directly from its docs source."""
import argparse
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
APP = ROOT / "examples/pinned-source"
DOC = ROOT / "docs/src/content/docs/pinned-source.md"
URL = "https://github.com/velvetmonkey/safemesh"
FILES = ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "src/main.rs")


def run(argv, **kwargs):
    return subprocess.run(argv, check=True, text=True, **kwargs)


def source_revision(app):
    lock = tomllib.loads((app / "Cargo.lock").read_text())
    package, = [p for p in lock["package"] if p["name"] == "safemesh-crdt"]
    match = re.fullmatch(r"git\+" + re.escape(URL) + r"\?rev=([0-9a-f]{40})#([0-9a-f]{40})", package["source"])
    if not match or match[1] != match[2]:
        raise ValueError("lockfile does not resolve the full requested source revision")
    return match[2]


def commands():
    result = "mkdir pinned-consumer\ncd pinned-consumer\nmkdir src\n"
    for name in FILES:
        result += f"cat > {name} <<'SAFEMESH_FILE'\n{(APP / name).read_text()}SAFEMESH_FILE\n"
    return result + "rustup show active-toolchain >&2\ncargo run --quiet --locked\n"


def pattern(kind):
    return rf"<!-- pinned:{kind} -->\n(.*?)<!-- /pinned:{kind} -->"


def extract(doc, kind, language):
    block, = re.findall(pattern(kind), doc, re.S)
    match = re.fullmatch(rf"```{language}\n(.*?)```\n", block, re.S)
    if not match:
        raise ValueError(f"invalid {kind} fence")
    return match[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--generate", metavar="REVISION")
    parser.add_argument("--work-parent", type=Path, default=ROOT / "target/pinned-source")
    args = parser.parse_args()
    doc = DOC.read_text()
    if args.generate:
        revision = run(["git", "rev-parse", "--verify", args.generate + "^{commit}"],
                       cwd=ROOT, capture_output=True).stdout.strip()
        (APP / "Cargo.toml").write_text(
            '[package]\nname = "safemesh-pinned-consumer"\nversion = "0.0.0"\n'
            'edition = "2021"\npublish = false\n\n[dependencies]\n'
            f'safemesh-crdt = {{ git = "{URL}", rev = "{revision}" }}\n')
        run(["cargo", "generate-lockfile"], cwd=APP)
        if source_revision(APP) != revision:
            raise ValueError("generated lockfile differs from requested revision")
        output = run(["cargo", "run", "--quiet", "--locked"], cwd=APP, capture_output=True).stdout
        for kind, language, content in (("revision", "text", revision + "\n"),
                                         ("commands", "sh", commands()), ("output", "text", output)):
            replacement = f"<!-- pinned:{kind} -->\n```{language}\n{content}```\n<!-- /pinned:{kind} -->"
            doc, count = re.subn(pattern(kind), lambda _: replacement, doc, flags=re.S)
            if count != 1:
                raise ValueError(f"expected one {kind} block")
        DOC.write_text(doc)
        print(f"Generated recipe from Cargo-resolved revision {revision}")
        return

    tested = source_revision(APP)
    manifest = tomllib.loads((APP / "Cargo.toml").read_text())["dependencies"]["safemesh-crdt"]
    command = extract(doc, "commands", "sh")
    if (extract(doc, "revision", "text") != tested + "\n"
            or manifest != {"git": URL, "rev": tested}
            or re.findall(r'rev = "([0-9a-f]{40})"', command) != [tested]):
        raise ValueError("SHA mismatch: documentation/dependency differs from tested lockfile revision")
    args.work_parent.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="consumer-", dir=args.work_parent.resolve()))
    home = work / "cargo-home"
    home.mkdir()
    env = os.environ.copy()
    # Prevent caller caches/configuration from changing this clean consumer build.
    for name in tuple(env):
        if name.startswith("CARGO_") or name in ("RUSTUP_TOOLCHAIN", "RUSTFLAGS", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"):
            del env[name]
    env["CARGO_HOME"] = str(home)
    print(f"Clean consumer: {work}; empty CARGO_HOME: {home}", flush=True)
    result = subprocess.run(["bash", "-euo", "pipefail", "-c", command], cwd=work,
                            env=env, text=True, capture_output=True)
    (work / "stdout").write_text(result.stdout)
    (work / "stderr").write_text(result.stderr)
    (work / "exit").write_text(str(result.returncode) + "\n")
    print(result.stdout, end="")
    if result.returncode:
        raise ValueError(f"documented command failed: exit {result.returncode}\n{result.stderr}")
    if result.stdout != extract(doc, "output", "text"):
        raise ValueError("documented stdout differs from expected output")
    consumer = work / "pinned-consumer"
    if source_revision(consumer) != tested:
        raise ValueError("SHA mismatch: executed consumer differs from tested revision")
    for name in FILES:
        if (consumer / name).read_bytes() != (APP / name).read_bytes():
            raise ValueError(f"documented {name} differs from committed example")
    if command != commands():
        raise ValueError("documented commands differ from generated recipe")
    print(f"PASS: revision={tested}; commands=9; stdout=2 lines; failures=0")
    print((consumer / "Cargo.lock").read_text(), end="")


if __name__ == "__main__":
    main()
