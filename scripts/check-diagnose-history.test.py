#!/usr/bin/env python3
"""Planted Diagnose prose and silence cases for the live Rust restart check."""

from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
PAGE = ROOT / "docs/src/content/docs/persist-and-restart.md"
CHECK = ROOT / "scripts/check-diagnose-history.py"


def check(page, *options):
    return subprocess.run(["python3", str(CHECK), "--page", str(page), *options],
                          cwd=ROOT, capture_output=True, text=True)


def main():
    source = PAGE.read_text()
    passed = 0
    with tempfile.TemporaryDirectory(prefix="diagnose-history-test-") as folder:
        page = Path(folder) / "page.md"
        page.write_text(source)
        t1 = check(page)
        results = [(t1.returncode == 0 and "DIAGNOSE HISTORY CASES 5, NAMED 5, MISSING 0" in t1.stdout, "T1")]

        start = source.index("`History(TrailingBytes)`", source.index("## Diagnose a failed Rust exercise"))
        end = source.index("`History(InvalidTag)`", start)
        page.write_text(source[:start] + source[end:])
        t2 = check(page)
        results.append((t2.returncode == 1 and "case append_frame_byte: History(TrailingBytes) on page no" in t2.stdout, "T2"))

        page.write_text(source.replace("unexpected trailing bytes after wire value", "unexpected extra bytes after wire value", 1))
        t3 = check(page)
        results.append((t3.returncode == 1 and "case append_frame_byte: History(TrailingBytes) on page no" in t3.stdout, "T3"))

        page.write_text(source)
        t4 = check(page, "--cases", "")
        results.append((t4.returncode == 1 and "DIAGNOSE HISTORY CASES 0" in t4.stdout, "T4"))

        for good, label in results:
            print(f"{'PASS' if good else 'FAIL'} {label}")
            passed += int(good)
        print(f"TESTS 4, PASSED {passed}")
        return int(passed != 4)


if __name__ == "__main__":
    raise SystemExit(main())
