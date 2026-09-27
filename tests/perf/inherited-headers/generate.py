#!/usr/bin/env python3
"""Generate a standard-library-only source with a wide inherited API."""

import argparse
from pathlib import Path


def write_fixture(path: Path, children: int = 50, members: int = 3000) -> None:
    lines = ["package inheritedheaders", "", "trait Wide {"]
    lines.extend(f"  def item{i}: Int" for i in range(members))
    lines.extend(["}", ""])
    for i in range(children):
        lines.extend([f"abstract class Wrap{i} extends Wide", ""])
    lines.extend(
        [
            "object InheritedHeaders {",
            "  def main(args: Array[String]): Unit = println(1)",
            "}",
        ]
    )
    path.write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--children", type=int, default=50)
    parser.add_argument("--members", type=int, default=3000)
    args = parser.parse_args()
    write_fixture(args.output, args.children, args.members)
