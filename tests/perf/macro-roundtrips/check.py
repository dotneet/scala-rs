#!/usr/bin/env python3
"""Check that repeated macro parsing stays faster than resident scalac."""

import argparse
import subprocess
import sys
from pathlib import Path


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("native", type=Path)
    parser.add_argument("scala_dist", type=Path)
    parser.add_argument("jdk", type=Path)
    args = parser.parse_args()
    subprocess.run(
        [
            sys.executable,
            str(ROOT / "tests/perf/process-lifetime/macro_check.py"),
            str(args.native.resolve()),
            str(args.scala_dist.resolve()),
            str(args.jdk.resolve()),
            "--provider",
            str(HERE / "ProbeMacro.scala"),
            "--source",
            str(HERE / "ManyQueries.scala"),
            "--main-class",
            "ManyQueries",
            "--expected-output",
            "672",
            "--resident-compiler",
            "--daemon-policy",
            "default",
            "--repeats",
            "5",
            "--maximum-ratio",
            "1.0",
        ],
        check=True,
    )


if __name__ == "__main__":
    main()
