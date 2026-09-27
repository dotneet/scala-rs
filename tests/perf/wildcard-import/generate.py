#!/usr/bin/env python3
"""Generate a single-source wildcard-import benchmark without dependencies."""

import argparse
from pathlib import Path


def source(members: int, uses: int, wildcard: bool) -> str:
    lines = ["object Catalog {"]
    lines.extend(f"  def item{i}: Int = {i}" for i in range(members))
    lines.append("}")
    for i in range(uses):
        lines.append(f"object Use{i} {{")
        if wildcard:
            lines.append("  import Catalog._")
            lines.append(f"  def value: Int = item{i % members}")
        else:
            lines.append(f"  def value: Int = Catalog.item{i % members}")
        lines.append("}")
    lines.extend(
        [
            "object Main {",
            "  def main(args: Array[String]): Unit = println(Use0.value)",
            "}",
        ]
    )
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--members", type=int, required=True)
    parser.add_argument("--uses", type=int, required=True)
    parser.add_argument("--qualified", action="store_true")
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    if args.members < 1 or args.uses < 1:
        parser.error("members and uses must be positive")
    args.output.write_text(source(args.members, args.uses, not args.qualified))


if __name__ == "__main__":
    main()
