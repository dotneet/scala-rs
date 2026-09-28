#!/usr/bin/env python3
"""Measure large, unused macro arguments against a resident scalac."""

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path


HERE = Path(__file__).resolve().parent
MACRO_CHECK = HERE.parent / "process-lifetime" / "macro_check.py"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("native", type=Path)
    parser.add_argument("scala_dist", type=Path)
    parser.add_argument("jdk", type=Path)
    parser.add_argument("--calls", type=int, default=200)
    parser.add_argument("--values", type=int, default=512)
    parser.add_argument("--repeats", type=int, default=4)
    parser.add_argument("--maximum-ratio", type=float, default=1.6)
    args = parser.parse_args()

    with tempfile.TemporaryDirectory(prefix="lazy-macro-arguments-") as directory:
        source = Path(directory) / "Use.scala"
        with source.open("w") as out:
            out.write("object Use {\n")
            for call in range(args.calls):
                out.write(f"  def p{call}: Int = ArgumentProbe.constant({{\n")
                for value in range(args.values):
                    out.write(f"    val item{value} = {value}\n")
                out.write(f"    item{args.values - 1}\n  }})\n")
            out.write("  def main(args: Array[String]): Unit = println(p0)\n}\n")
        command = [
            sys.executable, str(MACRO_CHECK), str(args.native),
            str(args.scala_dist), str(args.jdk), "--resident-native",
            "--resident-compiler", "--provider", str(HERE / "Provider.scala"),
            "--source", str(source), "--main-class", "Use",
            "--expected-output", "1", "--repeats", str(args.repeats),
            "--maximum-ratio", str(args.maximum_ratio),
        ]
        subprocess.run(command, check=True)


if __name__ == "__main__":
    main()
