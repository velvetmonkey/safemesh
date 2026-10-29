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
        # A real no-change restart succeeds. A typed list of expected names
        # would incorrectly claim a History error here.
        t5 = check(page, "--cases", "no_change")
        t5_good = (t5.returncode == 1 and
                   "case no_change: other on page no" in t5.stdout)
        print(f"{'PASS' if t5_good else 'FAIL'} T5")

        section_start = source.index("## Diagnose a failed Rust exercise")
        section_end = source.index("\n## ", section_start + 3)
        section = source[section_start:section_end]
        trailing = "unexpected trailing bytes after wire value"
        invalid = "unexpected wire tag"
        assert section.count(trailing) == section.count(invalid) == 1
        swapped = section.replace(trailing, "__DISPLAY_SWAP__")
        swapped = swapped.replace(invalid, trailing).replace("__DISPLAY_SWAP__", invalid)
        page.write_text(source[:section_start] + swapped + source[section_end:])
        t6 = check(page)
        t6_good = (t6.returncode == 1 and
                   "case append_frame_byte: History(TrailingBytes) on page no" in t6.stdout and
                   "case changed_tag: History(InvalidTag) on page no" in t6.stdout)
        print(f"{'PASS' if t6_good else 'FAIL'} T6")

        true_pairs = (f"\nHistory(TrailingBytes) (Display: local history wire validation failed: "
                      f"{trailing}). History(InvalidTag) (Display: local history wire "
                      f"validation failed: {invalid}).\n")
        page.write_text(source[:section_start] + swapped + true_pairs + source[section_end:])
        t7 = check(page)
        t7_good = (t7.returncode == 1 and
                   "case append_frame_byte: History(TrailingBytes) on page no" in t7.stdout and
                   "case changed_tag: History(InvalidTag) on page no" in t7.stdout)
        print(f"{'PASS' if t7_good else 'FAIL'} T7")
        total_passed = passed + int(t5_good) + int(t6_good) + int(t7_good)
        print(f"TESTS 7, PASSED {total_passed}")
        return int(total_passed != 7)


if __name__ == "__main__":
    raise SystemExit(main())
