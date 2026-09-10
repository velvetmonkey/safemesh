# Copyright (C) 2026 Ben Cassie
# SPDX-License-Identifier: Apache-2.0
"""Regression rows copied from the original instrument, with no changed measurements."""
import unittest
from pathlib import Path
from unittest.mock import patch
import summarize

B7_HEAD = {'batch': 1, 'byte_cap': 96, 'crate_version': '0.1.0', 'family': 'B7', 'n': 8, 'payload': 32, 'repetitions': 7, 'seed': 42, 'writers': 1}
B7_ROW = {'allocation_bytes': 5759, 'allocation_calls': 80, 'allocator_extra_peak_bytes': 2854, 'allocator_peak_bytes': 5835, 'assertion': 'durable G-Counter bump survives restart', 'condition': 'warm', 'history_encoded_bytes': 396, 'history_records': 8, 'kernel_write_bytes': 4096, 'ns': 856345, 'op': 'durable_counter_bump', 'payload_bytes': 8, 'process_peak_rss_bytes': 2543616, 'rep': 0, 'serialized_write_amplification': 57.75, 'transaction_bytes': 462}


class Labels(unittest.TestCase):
    def test_recorded_b7_is_durable_gcounter(self):
        meta = {"name": "B7-r0", "metadata": {"source_sha": "d73daa2d7b39ae5dfb16022e16b45412f237c1ae"}}
        with patch.object(summarize, "load", return_value=([(meta, B7_HEAD, [B7_ROW])], [])):
            output = summarize.generate(Path("unused"))
        self.assertIn("G-Counter; durable; durable_counter_bump", output)
        self.assertNotIn("Carriers NOT MEASURED: G-Set, G-Counter", output)

    def test_unseen_identity_and_family(self):
        row = {**B7_ROW, "crdt_type": "PN-Counter", "durability": "in-memory",
               "op": "merge_delta", "assertion": "synthetic unseen schema row"}
        head = {**B7_HEAD, "family": "UNSEEN"}
        meta = {"name": "holdout", "metadata": {"source_sha": "synthetic"}}
        with patch.object(summarize, "load", return_value=([(meta, head, [row])], [])):
            output = summarize.generate(Path("unused"))
        self.assertIn("PN-Counter; in-memory; merge_delta", output)
        self.assertNotIn("G-Counter; durable", output)

    def test_missing_identity_does_not_guess_from_operation_or_writes(self):
        for writes in (0, 4096):
            row = {"op": "append", "kernel_write_bytes": writes}
            self.assertEqual(summarize.measurement_label(row),
                             "CRDT not recorded; durability not recorded; append")

    def test_explicit_identity_wins_over_legacy_assertion(self):
        row = {**B7_ROW, "crdt_type": "OR-Set UTF-8", "durability": "in-memory"}
        self.assertEqual(summarize.identity(row), ("OR-Set UTF-8", "in-memory"))


if __name__ == "__main__":
    unittest.main()
