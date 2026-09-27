#!/usr/bin/env python3
"""Check the flat Block macro transport against a resident scalac process."""

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("native", type=Path)
    parser.add_argument("scala_dist", type=Path)
    parser.add_argument("jdk", type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="macro-block-chain-") as scratch:
        source = Path(scratch) / "ChainUse.scala"
        subprocess.run([sys.executable, str(HERE / "generate.py"), str(source)], check=True)
        subprocess.run(
            [
                sys.executable,
                str(ROOT / "tests/perf/process-lifetime/macro_check.py"),
                str(args.native.resolve()),
                str(args.scala_dist.resolve()),
                str(args.jdk.resolve()),
                "--provider",
                str(ROOT / "tests/fixtures/macroblockchild_impl.scala"),
                "--source",
                str(source),
                "--main-class",
                "ChainUse",
                "--expected-output",
                "true",
                "--resident-compiler",
                "--daemon-policy",
                "required",
                "--repeats",
                "4",
                "--maximum-ratio",
                "1.55",
            ],
            check=True,
        )


if __name__ == "__main__":
    main()
