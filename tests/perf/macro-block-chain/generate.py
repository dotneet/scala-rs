#!/usr/bin/env python3
"""Generate a dependency-free client with a long, flat macro-produced Block."""

import argparse
from pathlib import Path


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--calls", type=int, default=300)
    args = parser.parse_args()
    if args.calls < 1:
        parser.error("--calls must be positive")
    chain = "(new Chain)" + ".append[Int]" * args.calls
    args.output.write_text(
        "object ChainUse {\n"
        f"  val result: Chain = {chain}\n"
        "  def main(args: Array[String]): Unit = println(result != null)\n"
        "}\n"
    )


if __name__ == "__main__":
    main()
