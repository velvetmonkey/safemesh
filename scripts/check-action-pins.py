#!/usr/bin/env python3
"""Check that every workflow action uses a fixed commit."""

from pathlib import Path
import re


WORKFLOWS = Path(__file__).resolve().parent.parent / ".github" / "workflows"
USES = re.compile(r"^\s*(?:-\s*)?uses:\s*(\S+)")
PINNED = re.compile(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*@[0-9a-f]{40}\Z")


def main() -> int:
    checked = 0
    failed = False
    for workflow in sorted((*WORKFLOWS.glob("*.yml"), *WORKFLOWS.glob("*.yaml"))):
        for line_number, line in enumerate(workflow.read_text().splitlines(), 1):
            match = USES.match(line)
            if match is None:
                continue
            checked += 1
            value = match.group(1)
            unquoted = value
            if len(value) >= 2 and value[0] == value[-1] and value[0] in "\"'":
                unquoted = value[1:-1]
            if not (unquoted.startswith("./") or PINNED.fullmatch(unquoted)):
                print(f"{workflow}:{line_number}: unpinned action {value}")
                failed = True
    if failed:
        return 1
    if checked == 0:
        print(f"No workflow uses lines found under {WORKFLOWS}")
        return 1
    print(f"Checked {checked} uses lines")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
