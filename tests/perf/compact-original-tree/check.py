#!/usr/bin/env python3
"""Measure repeated large macro receiver trees against resident scalac."""

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
    parser.add_argument("--chains", type=int, default=4)
    parser.add_argument("--depth", type=int, default=32)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--maximum-ratio", type=float, default=2.0)
    args = parser.parse_args()

    with tempfile.TemporaryDirectory(prefix="compact-original-tree-") as directory:
        source = Path(directory) / "Use.scala"
        with source.open("w") as out:
            out.write("object Use {\n")
            for chain in range(args.chains):
                calls = ".append(1)" * args.depth
                out.write(f"  def value{chain}: ChainValue = new ChainValue(0){calls}\n")
            out.write("  def main(args: Array[String]): Unit = {\n")
            out.write("    println(value0.depth)\n")
            out.write("    println(new ChainValue(1).duplicate)\n")
            out.write("  }\n}\n")
        command = [
            sys.executable, str(MACRO_CHECK), str(args.native),
            str(args.scala_dist), str(args.jdk), "--resident-native",
            "--resident-compiler", "--provider", str(HERE / "Provider.scala"),
            "--source", str(source), "--main-class", "Use",
            "--expected-output", f"{args.depth}\n2", "--repeats", str(args.repeats),
            "--maximum-ratio", str(args.maximum_ratio),
        ]
        subprocess.run(command, check=True)


if __name__ == "__main__":
    main()
