#!/usr/bin/env python3
"""Compare nested implicit search with many unrelated binary candidates."""

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
NOISE = 1000
DEPTH = 12


def provider_source() -> str:
    lines = [
        "package typeclassprobe",
        "trait Evidence[A]",
        "final class Link[A]",
    ]
    lines.extend(f"trait Missing{i}[A]" for i in range(NOISE))
    lines.extend([
        "object Instances {",
        "  implicit val base: Evidence[Int] = new Evidence[Int] {}",
        "  implicit def link[A](implicit e: Evidence[A]): Evidence[Link[A]] = new Evidence[Link[A]] {}",
    ])
    lines.extend(
        f"  implicit def noise{i}[A](implicit e: Missing{i}[A]): Evidence[A] = new Evidence[A] {{}}"
        for i in range(NOISE)
    )
    lines.append("}")
    return "\n".join(lines) + "\n"


def client_source() -> str:
    target = "Int"
    for _ in range(DEPTH):
        target = f"Link[{target}]"
    return (
        "import typeclassprobe._\n"
        "import typeclassprobe.Instances._\n"
        "object Work {\n"
        f"  val evidence = implicitly[Evidence[{target}]]\n"
        "  def main(args: Array[String]): Unit = println(if (evidence == null) 0 else 1)\n"
        "}\n"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("native", type=Path)
    parser.add_argument("scala_dist", type=Path)
    parser.add_argument("jdk", type=Path)
    parser.add_argument("--repeats", type=int, default=4)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="implicit-head-index-") as scratch:
        provider = Path(scratch) / "Provider.scala"
        client = Path(scratch) / "Client.scala"
        provider.write_text(provider_source())
        client.write_text(client_source())
        subprocess.run(
            [
                sys.executable,
                str(ROOT / "perf/process-lifetime/macro_check.py"),
                str(args.native),
                str(args.scala_dist),
                str(args.jdk),
                "--provider", str(provider),
                "--source", str(client),
                "--main-class", "Work",
                "--expected-output", "1",
                "--resident-compiler",
                "--daemon-policy", "default",
                "--repeats", str(args.repeats),
                "--maximum-ratio", "1.0",
            ],
            check=True,
        )


if __name__ == "__main__":
    main()
