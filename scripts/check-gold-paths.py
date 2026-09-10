#!/usr/bin/env python3
"""Execute the documented consumer commands and compare their output and source blocks.

--refresh regenerates expected stdout by running the asserting programs; review the diff.
--write-docs copies fixture blocks into Markdown. Neither mode is used in CI.
"""
import argparse
import difflib
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "examples/gold-path"
COMMANDS = json.loads((FIXTURES / "commands.json").read_text())


def fixture_block(kind, name):
    if kind == "commands":
        return "```sh\n" + "\n".join(item["command"] for item in COMMANDS[name]) + "\n```"
    if kind == "output":
        return "```text\n" + "".join((FIXTURES / item["output"]).read_text()
                                     for item in COMMANDS[name] if "output" in item) + "```"
    language, path = name.split(":", 1)
    return f"```{language}\n{(FIXTURES / path).read_text()}```"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--refresh", action="store_true")
    parser.add_argument("--write-docs", action="store_true")
    args = parser.parse_args()
    failures = []
    count = 0
    logs = Path(os.environ.get("GOLD_PATH_LOGS", FIXTURES / ".checks"))
    logs.mkdir(parents=True, exist_ok=True)
    if not args.write_docs:
        for lane, commands in COMMANDS.items():
            for index, item in enumerate(commands, 1):
                count += 1
                label = f"{lane}-{index}"
                result = subprocess.run(item["command"], shell=True, executable="/bin/bash",
                                        cwd=ROOT, text=True, capture_output=True)
                (logs / f"{label}.stdout").write_text(result.stdout)
                (logs / f"{label}.stderr").write_text(result.stderr)
                exit_file = logs / f"{label}.exit"
                exit_file.write_text(f"{result.returncode}\n")
                code = int(exit_file.read_text())
                print(f"{label}: exit {code}: {item['command']}", flush=True)
                if code:
                    failures.append(f"{label}: command failed\n{result.stderr}")
                    break  # this path's later commands depend on this one
                if "output" in item:
                    expected = FIXTURES / item["output"]
                    if args.refresh:
                        expected.write_text(result.stdout)
                    elif expected.read_text() != result.stdout:
                        failures.append(f"{label}: stdout drift\n" + "".join(difflib.unified_diff(
                            expected.read_text().splitlines(True), result.stdout.splitlines(True),
                            fromfile=str(expected), tofile="actual stdout")))
    if args.refresh:
        for failure in failures:
            print(failure)
        print(f"refreshed stdout: commands={count} failures={len(failures)}")
        return bool(failures)
    pattern = re.compile(r"(<!-- gold:(commands|output|source) ([^ ]+) -->\n)(.*?)(\n<!-- /gold -->)", re.S)
    blocks = 0
    for filename in ("getting-started.md", "using-safemesh.md"):
        page = ROOT / "docs/src/content/docs" / filename
        def replace(match):
            nonlocal blocks
            blocks += 1
            block = fixture_block(match[2], match[3])
            if not args.write_docs and match[4] != block:
                failures.append(f"{filename}: fixture block drift: {match[2]} {match[3]}")
            return match[1] + block + match[5]
        updated = pattern.sub(replace, page.read_text())
        if args.write_docs:
            page.write_text(updated)
    if blocks != 8:
        failures.append(f"expected 8 fixture blocks, found {blocks}")
    for failure in failures:
        print(failure)
    print(f"gold paths: commands={count} failures={len(failures)} fixture-blocks={blocks}")
    return bool(failures)


if __name__ == "__main__":
    raise SystemExit(main())
