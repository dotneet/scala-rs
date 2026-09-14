#!/usr/bin/env python3
"""Lightweight unit tests for the merge-gate result protocol."""

import json
import tempfile
import unittest
from pathlib import Path

from gate_result import ResultError, read, read_row, write


class GateResultTest(unittest.TestCase):
    def test_round_trip_and_fixed_row(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "step.json"
            write(path, "tests", "pass", 0, "rows=2 classes=3")
            self.assertEqual(
                read(path),
                {
                    "schema": 1,
                    "step": "tests",
                    "status": "pass",
                    "exit_code": 0,
                    "summary": "rows=2 classes=3",
                },
            )
            self.assertEqual(read_row(path, "tests").split("\t")[:3], ["tests", "pass", "0"])

    def test_write_rejects_bad_values(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "step.json"
            for args in (
                ("bad step", "pass", 0, "ok"),
                ("tests", "unknown", 0, "ok"),
                ("tests", "pass", 256, "ok"),
                ("tests", "pass", 0, "two\nlines"),
            ):
                with self.subTest(args=args), self.assertRaises(ResultError):
                    write(path, *args)

    def test_read_rejects_missing_extra_and_malformed_fields(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "step.json"
            good = {"schema": 1, "step": "tests", "status": "pass", "exit_code": 0, "summary": "ok"}
            for record in (
                {key: value for key, value in good.items() if key != "summary"},
                {**good, "unexpected": True},
                {**good, "exit_code": "0"},
                {**good, "schema": 99},
            ):
                path.write_text(json.dumps(record), encoding="utf-8")
                with self.subTest(record=record), self.assertRaises(ResultError):
                    read(path, "tests")


if __name__ == "__main__":
    unittest.main()
