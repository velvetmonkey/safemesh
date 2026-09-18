#!/usr/bin/env python3
"""Compile and run the CRDT README quickstart against its documented Git pin."""

import argparse
from pathlib import Path
import re
import subprocess
import tomllib


ROOT = Path(__file__).resolve().parents[1]


def code_block(markdown, heading, language):
    sections = re.findall(
        rf"^## {re.escape(heading)}\n(.*?)(?=^## |\Z)",
        markdown,
        re.MULTILINE | re.DOTALL,
    )
    if len(sections) != 1:
        raise ValueError(f"expected one {heading} section")
    blocks = re.findall(
        rf"^```{re.escape(language)}\n(.*?)^```$",
        sections[0],
        re.MULTILINE | re.DOTALL,
    )
    if len(blocks) != 1:
        raise ValueError(f"expected one {language} block in {heading}")
    return blocks[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work-dir", type=Path, required=True,
                        help="directory for the generated consumer and Cargo artifacts")
    args = parser.parse_args()
    readme = (ROOT / "rust/crates/safemesh-crdt/README.md").read_text()
    install = code_block(readme, "Install", "toml")
    quickstart = code_block(readme, "Quickstart", "rust")
    dependency = tomllib.loads(install)["dependencies"]["safemesh-crdt"]
    if "git" not in dependency or not re.fullmatch(r"[0-9a-f]{40}", dependency.get("rev", "")):
        raise ValueError("README dependency must specify a Git URL and full revision")
    consumer = args.work_dir.resolve()
    (consumer / "src").mkdir(parents=True, exist_ok=True)
    (consumer / "Cargo.toml").write_text(
        '[package]\nname = "readme-quickstart"\nversion = "0.0.0"\nedition = "2021"\n'
        '[workspace]\n' + install
    )
    # Like a rustdoc example, the README's statements need only a main wrapper.
    # Do not rewrite the snippet or substitute a checkout-local dependency.
    (consumer / "src/main.rs").write_text("fn main() {\n" + quickstart + "}\n")
    print(f"README quickstart: {dependency['git']} @ {dependency['rev']}", flush=True)
    result = subprocess.run(["cargo", "run", "--manifest-path", str(consumer / "Cargo.toml")])
    exit_file = consumer / "cargo.exit"
    exit_file.write_text(f"{result.returncode}\n")
    return int(exit_file.read_text())


if __name__ == "__main__":
    raise SystemExit(main())
