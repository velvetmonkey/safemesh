#!/usr/bin/env python3
"""Plant isolated documentation and implementation drift cases."""

from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
FILES = ("demos/two-app-inventory/README.md",
         "demos/two-app-inventory/node.py",
         "demos/two-app-inventory/test_sync.py")
WARNING = (" A store that an earlier version of this demo wrote before PR 344 "
           "(for example, the v0.2.1 tag) has no frame on its rows and does not "
           "open after upgrade. Startup stops with `stored record 1 failed its "
           "frame check`. The demo has no migration, so start a new store.")


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
        self.assertIn(f"TWO-APP UPGRADE PARTS 4, MISSING {0 if expected == 0 else 1}",
                      result.stdout)

    def test_t1_head_copies_pass(self):
        self.check(0)

    def test_t2_warning_removed(self):
        self.replace(FILES[0], WARNING, "")
        self.check(1)

    def test_t3_readme_refusal_drift(self):
        self.replace(FILES[0], "failed its frame check", "failed its digest check")
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
