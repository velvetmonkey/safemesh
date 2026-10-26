#!/usr/bin/env python3
"""Plant isolated documentation and implementation drift cases."""

from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
FILES = ("demos/two-app-inventory/README.md",
         "demos/two-app-inventory/node.py",
         "demos/two-app-inventory/test_sync.py")
class UpgradeCheckTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        for relative in FILES:
            target = self.root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / relative, target)

    def replace(self, relative, old, new):
        path = self.root / relative
        text = path.read_text()
        self.assertIn(old, text)
        path.write_text(text.replace(old, new, 1))

    def check(self, expected):
        result = subprocess.run(
            [sys.executable, "-I", str(ROOT / "scripts/check-two-app-upgrade.py"),
             "--root", str(self.root)], capture_output=True, text=True)
        self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
        if expected == 0:
            self.assertIn("TWO-APP UPGRADE PARTS 4, MISSING 0", result.stdout)
        else:
            self.assertRegex(result.stdout, r"TWO-APP UPGRADE PARTS 4, MISSING [1-4]")

    def test_t1_head_copies_pass(self):
        self.check(0)

    def test_t2_warning_removed(self):
        path = self.root / FILES[0]
        text, count = re.subn(r" A store that an earlier version.*?start a new store\.",
                              "", path.read_text(), count=1, flags=re.DOTALL)
        self.assertEqual(count, 1)
        path.write_text(text)
        self.check(1)

    def test_t3_readme_refusal_drift(self):
        self.replace(FILES[0], "failed its\n  frame check", "failed its\n  digest check")
        self.check(1)

    def test_t4_node_refusal_drift(self):
        self.replace(FILES[1], "failed its frame check", "failed its digest check")
        self.check(1)

    def test_t5_migration_dropped(self):
        self.replace(FILES[0], "The demo has no migration", "The demo starts a new store")
        self.check(1)

    def test_t6_legacy_assertion_dropped(self):
        self.replace(FILES[2], "accepted legacy row", "accepted old row")
        self.check(1)


if __name__ == "__main__":
    unittest.main()
