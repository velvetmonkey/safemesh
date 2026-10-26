#!/usr/bin/env python3
"""Run the retention benchmark and check every bounded CI-size result row."""
import argparse
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
EXPECTED = {(kind, size) for kind in ("gcounter", "utf8-orset") for size in (1000, 10000)}
NUMBER = r"([0-9]+(?:\.[0-9]+)?)"


def number(cell: str, suffix: str) -> float:
    match = re.fullmatch(NUMBER + re.escape(suffix), cell)
    if not match:
        raise ValueError(f"invalid value {cell!r}")
    return float(match.group(1))


def inspect(path: Path) -> int:
    problems = []
    rows = {"append": {}, "restart": {}}
    section = None
    if not path.is_file() or not path.read_text().strip():
        print("RETENTION BUDGET ROWS 0: absent or empty benchmark output")
        print("RETENTION BUDGET ROWS 0, VALUES 0, OVER 1")
        return 1
    for line in path.read_text().splitlines():
        if line.startswith("### Append cost:"):
            section = "append"
        elif line.startswith("### Append-log store: restart and size"):
            section = "restart"
        elif line.startswith("### "):
            section = None
        if section is None or not line.startswith("|") or line.startswith(("| CRDT", "|---")):
            continue
        cells = [part.strip() for part in line.strip("|").split("|")]
        expected_columns = 10 if section == "append" else 8
        if len(cells) != expected_columns:
            problems.append(f"{section} row: expected {expected_columns} columns, got {len(cells)}")
            continue
        try:
            key = (cells[0], int(cells[1]))
        except ValueError:
            problems.append(f"{section} row: invalid size {cells[1]!r}")
            continue
        if key in rows[section]:
            problems.append(f"{section} {key[0]} {key[1]}: duplicate row")
        else:
            rows[section][key] = cells

    for section in ("append", "restart"):
        keys = set(rows[section])
        if keys != EXPECTED:
            problems.append(
                f"{section} ROWS {len(keys)}: expected 4; missing {sorted(EXPECTED - keys)}; extra {sorted(keys - EXPECTED)}"
            )

    budgets = [
        ("restart", 2, "restart", " ms", 500),
        ("restart", 6, "restart after migration", " ms", 500),
        ("restart", 3, "peak RSS", " MiB", 64),
        ("restart", 7, "peak RSS after migration", " MiB", 64),
        ("append", 6, "after-append bytes written/record", "", 256),
        ("restart", 5, "disk bytes/record", "", 128),
        ("append", 8, "after-append median append", " ms", 50),
    ]
    values = 0
    for key in sorted(EXPECTED):
        for section, column, label, suffix, limit in budgets:
            cells = rows[section].get(key)
            if cells is None:
                continue
            values += 1
            try:
                value = number(cells[column], suffix)
            except ValueError as error:
                problems.append(f"{key[0]} {key[1]} {label}: {error}")
                continue
            if value > limit:
                problems.append(f"{key[0]} {key[1]} {label}: {value:g} > {limit:g}")
    for problem in problems:
        print(f"RETENTION BUDGET {problem}")
    complete = len(set(rows["append"]) & set(rows["restart"]) & EXPECTED)
    print(f"RETENTION BUDGET ROWS {complete}, VALUES {values}, OVER {len(problems)}")
    return int(bool(problems))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--input", type=Path, help="Read a measured output copy instead of running cargo")
    parser.add_argument("--benchmark-command", nargs="+", help="Test-only command; CI never passes this")
    args = parser.parse_args()
    if args.benchmark_command:
        result = subprocess.run(args.benchmark_command, check=False)
        if result.returncode:
            print(f"RETENTION BUDGET benchmark exit {result.returncode}")
            print("RETENTION BUDGET ROWS 0, VALUES 0, OVER 1")
            return 1
    if args.input is not None:
        return inspect(args.input)
    with tempfile.TemporaryDirectory(prefix="safemesh-retention-budget-", dir=os.environ.get("TMPDIR")) as private:
        folder = Path(private)
        command = [
            "cargo", "run", "--release", "--locked", "-q", "-p", "safemesh-crdt",
            "--features", "local-writer", "--example", "retention_bench", "--",
            "--sizes", "1000,10000", "--runs", "3", "--appends", "10",
            "--dir", str(folder / "bench"), "--out", str(folder / "result.md"),
        ]
        try:
            result = subprocess.run(command, cwd=ROOT / "rust", check=False, timeout=300)
        except subprocess.TimeoutExpired:
            print("RETENTION BUDGET benchmark timeout after 300 seconds")
            print("RETENTION BUDGET ROWS 0, VALUES 0, OVER 1")
            return 1
        if result.returncode:
            print(f"RETENTION BUDGET benchmark exit {result.returncode}")
            print("RETENTION BUDGET ROWS 0, VALUES 0, OVER 1")
            return 1
        return inspect(folder / "result.md")


if __name__ == "__main__":
    sys.exit(main())
