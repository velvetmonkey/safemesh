#!/usr/bin/env python3
"""Execute the documented consumer commands and compare their output and source blocks.

--refresh regenerates expected stdout by running the asserting programs; review the diff.
--write-docs copies fixture blocks into Markdown. Neither mode is used in CI.
Fence validation requires the site's parser dependencies: npm --prefix docs ci.
"""
import argparse
import difflib
import json
import os
from pathlib import Path
import re
import subprocess
import shutil
import tempfile

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "examples/gold-path"
COMMANDS = json.loads((FIXTURES / "commands.json").read_text())


def check_fence(block):
    # Use the site's Markdown parser, independently of the fixture comparison.
    # A paragraph after the fence must remain outside the single code node.
    result = subprocess.run([
        "node", "--input-type=module", "-e", r"""
import {unified} from 'unified';
import remarkParse from 'remark-parse';
let block = '';
for await (const chunk of process.stdin) block += chunk;
const following = 'Gold fence boundary sentinel.';
const nodes = unified().use(remarkParse).parse(block + '\n\n' + following + '\n').children;
const closed = nodes.length === 2 && nodes[0].type === 'code'
    && nodes[0].position.end.line === block.split('\n').length
    && nodes[1].type === 'paragraph' && nodes[1].children.length === 1
    && nodes[1].children[0].type === 'text' && nodes[1].children[0].value === following;
process.stdout.write(JSON.stringify(closed));
"""], cwd=ROOT / "docs", input=block, text=True, capture_output=True, check=True)
    if not json.loads(result.stdout):
        raise ValueError("generated fixture must parse as one closed code block")


def fixture_block(kind, name):
    if kind == "commands":
        language = "sh"
        content = "\n".join(item["command"] for item in COMMANDS[name]) + "\n"
    elif kind == "output":
        language = "text"
        content = "".join((FIXTURES / item["output"]).read_text()
                          for item in COMMANDS[name] if "output" in item)
    else:
        language, path = name.split(":", 1)
        content = (FIXTURES / path).read_text()
    separator = "" if content.endswith("\n") else "\n"
    block = f"```{language}\n{content}{separator}```"
    check_fence(block)
    return block


PAGES = ("getting-started.md", "persist-and-restart.md", "connect-replicas.md", "using-safemesh.md")
PATTERN = re.compile(r"(<!-- gold:(commands|output|source) ([^ ]+) -->\n)(.*?)(\n<!-- /gold -->)", re.S)


def required_assertions(staging):
    """Printed examples alone must not replace the journey's semantic assertions."""
    required = {
        "rust/src/main.rs": (
            'assert_eq!(counter.state().value(),6);',
            'assert_eq!(members.state().elements(),["compass".to_string(),"map".to_string(),"rope".to_string()].into_iter().collect());',
        ),
        "typescript/main.ts": (
            'assert.equal(counter.value(),6n);',
            'assert.deepEqual(members.elements(),["compass","map","rope"]);',
        ),
    }
    failures = []
    for name, assertions in required.items():
        source = (staging / "examples/gold-path" / name).read_text()
        source = re.sub(r"/\*.*?\*/|//[^\n]*", "", source, flags=re.S)
        compact = re.sub(r"\s+", "", source)
        # Rustfmt permits a trailing comma in an assertion's argument list.
        compact = compact.replace(',);', ');')
        for assertion in assertions:
            if assertion not in compact:
                failures.append(f"{name}: missing required assertion: {assertion}")
    return failures


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--refresh", action="store_true")
    parser.add_argument("--write-docs", action="store_true")
    args = parser.parse_args()
    failures = []
    count = 0
    logs = Path(os.environ.get("GOLD_PATH_LOGS", FIXTURES / ".checks")).resolve()
    logs.mkdir(parents=True, exist_ok=True)
    commands = COMMANDS
    outputs = {}
    staging = ROOT
    blocks = 0
    if not args.refresh:
        # Materialize exactly the fenced sources, commands and outputs readers see.
        # Retain the sandbox for diagnosing failures, never overwrite the checkout.
        if not args.write_docs:
            staging = Path(tempfile.mkdtemp(prefix="extracted-", dir=logs))
            (staging / "examples").mkdir()
            shutil.copytree(FIXTURES, staging / "examples/gold-path",
                            ignore=shutil.ignore_patterns(".checks", "target", "pkg", "node_modules", "main.js"))
            (staging / "rust").symlink_to(ROOT / "rust", target_is_directory=True)
            shutil.copy2(ROOT / "rust-toolchain.toml", staging)
        extracted_commands = {}
        for filename in PAGES:
            page = ROOT / "docs/src/content/docs" / filename
            text = page.read_text()
            # Every fence on the three tutorial pages must belong to the mechanism.
            # check_fence uses the site's parser to verify each block is closed.
            remainder = PATTERN.sub("", text)
            if filename != "using-safemesh.md" and re.search(r"^\s*(`{3,}|~{3,})", remainder, re.M):
                failures.append(f"{filename}: untested code fence")
            def replace(match):
                nonlocal blocks
                blocks += 1
                expected = fixture_block(match[2], match[3])
                if not args.write_docs:
                    if match[4] != expected:
                        failures.append(f"{filename}: fixture block drift: {match[2]} {match[3]}")
                    check_fence(match[4])
                    content = match[4].split("\n", 1)[1].rsplit("\n```", 1)[0] + "\n"
                    kind, name = match[2], match[3]
                    if kind == "source":
                        _, path = name.split(":", 1)
                        (staging / "examples/gold-path" / path).write_text(content)
                    elif kind == "commands":
                        extracted_commands[name] = content.splitlines()
                    else:
                        outputs[name] = content
                return match[1] + expected + match[5]
            updated = PATTERN.sub(replace, text)
            if args.write_docs:
                page.write_text(updated)
        if blocks != 20:
            failures.append(f"expected 20 fixture blocks, found {blocks}")
        if not args.write_docs:
            if extracted_commands.keys() != COMMANDS.keys() or outputs.keys() != COMMANDS.keys():
                failures.append("every command group needs an extracted command and output block")
            commands = {name: [dict(item, command=command) for item, command in
                        zip(COMMANDS[name], lines, strict=True)]
                        for name, lines in extracted_commands.items()}
            failures.extend(required_assertions(staging))
    # Fail before execution if an assertion, source block or fence was removed.
    if not args.write_docs and not failures:
        for lane, items in commands.items():
            stdout = ""
            for index, item in enumerate(items, 1):
                count += 1
                label = f"{lane}-{index}"
                result = subprocess.run(item["command"], shell=True, executable="/bin/bash",
                                        cwd=staging, text=True, capture_output=True)
                (logs / f"{label}.stdout").write_text(result.stdout)
                (logs / f"{label}.stderr").write_text(result.stderr)
                exit_file = logs / f"{label}.exit"
                exit_file.write_text(f"{result.returncode}\n")
                code = int(exit_file.read_text())
                print(f"{label}: exit {code}: {item['command']}", flush=True)
                if code:
                    failures.append(f"{label}: command failed\n{result.stderr}")
                    break
                if "output" in item:
                    stdout += result.stdout
                    expected = FIXTURES / item["output"]
                    if args.refresh:
                        expected.write_text(result.stdout)
                    elif expected.read_text() != result.stdout:
                        failures.append(f"{label}: stdout drift\n" + "".join(difflib.unified_diff(
                            expected.read_text().splitlines(True), result.stdout.splitlines(True),
                            fromfile=str(expected), tofile="actual stdout")))
            if not args.refresh and stdout != outputs[lane]:
                failures.append(f"{lane}: executed stdout disagrees with extracted output")
    for failure in failures:
        print(failure)
    print(f"gold paths: commands={count} failures={len(failures)} fixture-blocks={blocks}")
    return bool(failures)


if __name__ == "__main__":
    raise SystemExit(main())
