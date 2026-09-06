"""Status gate regressions; no corpus checkout or compiler required."""

import tempfile
from pathlib import Path
import sys
import unittest

sys.dont_write_bytecode = True
from compare_corpus import compare, read_ledger


class CorpusComparisonTest(unittest.TestCase):
    def test_equal_totals_do_not_hide_swapped_successes(self):
        before = {("run", "first"): "pass", ("run", "second"): "fail"}
        after = {("run", "first"): "fail", ("run", "second"): "pass"}
        result = compare(before, after)
        self.assertEqual(result["baseline"], result["candidate"])
        self.assertEqual(result["losses"], 1)
        self.assertEqual(len(result["changes"]), 2)

    def test_skipping_failure_is_not_an_improvement(self):
        self.assertEqual(compare({("neg", "x"): "fail"}, {("neg", "x"): "skip"})["losses"], 1)

    def test_replaced_identity_is_incomparable(self):
        with self.assertRaisesRegex(ValueError, "different test identities"):
            compare({("pos", "old"): "pass"}, {("pos", "new"): "pass"})

    def test_diagnostic_bytes_are_not_row_delimiters(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "corpus.tsv"
            path.write_bytes(b"neg\tx\tpass\terror\tfirst\x1esecond\xff\treference\n")
            self.assertEqual(read_ledger(path), {("neg", "x"): "pass"})
            path.write_bytes(path.read_bytes() * 2)
            with self.assertRaisesRegex(ValueError, "duplicate identity"):
                read_ledger(path)

    def test_truncated_or_mixed_ledger_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "corpus.tsv"
            for data in (b"", b"pos\tx\tpass\tpartial\n",
                         b"pos\tx\tpass\npos\ty\tpass\t-\t\t\n"):
                path.write_bytes(data)
                with self.assertRaises(ValueError):
                    read_ledger(path)


if __name__ == "__main__":
    unittest.main()
