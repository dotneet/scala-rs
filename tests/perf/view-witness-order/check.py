#!/usr/bin/env python3
"""Compare early rejection of a view with an absent first witness."""

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
COUNT = 300


def provider_source() -> str:
    lines = [
        "package viewprobe",
        "trait Rep[A]",
        "final class Query[A, B, C] extends Rep[List[B]]",
        "final class Applied[PU, R <: Rep[_], RU]",
        "final class Compiled[F, PT, PU, R <: Rep[_], RU] {",
        "  def apply(value: PU): Applied[PU, R, RU] = new Applied[PU, R, RU]",
        "}",
        "final class ResultOps[A](value: A) { def result: Int = 1 }",
        "trait Other[A]",
    ]
    lines.extend(f"trait Missing{i}[A]" for i in range(COUNT))
    lines.extend(f"trait OtherMissing{i}[A]" for i in range(COUNT))
    lines.append("object Other {")
    lines.extend(
        f"  implicit def rule{i}[A](implicit ev: OtherMissing{i}[A]): Other[A] = null"
        for i in range(COUNT)
    )
    lines.extend([
        "}",
        "trait RootAPI[Cfg] {",
        "  implicit def good[PU, A, B, C, RU](value: Applied[PU, Query[A, B, C], RU]): "
        "ResultOps[(Cfg, Applied[PU, Query[A, B, C], RU])] = new ResultOps(null)",
    ])
    lines.extend(
        f"  implicit def noise{i}[PU, A, B, C, RU, X]"
        f"(value: Applied[PU, Query[A, B, C], RU])"
        f"(implicit ev: Missing{i}[X], extra: Other[X]): "
        f"ResultOps[(Cfg, Applied[PU, Query[A, B, C], RU], X)] = new ResultOps(null)"
        for i in range(COUNT)
    )
    lines.extend([
        "}",
        "trait MiddleAPI[Cfg] extends RootAPI[Cfg]",
        "object API extends MiddleAPI[String]",
    ])
    return "\n".join(lines) + "\n"


def client_source() -> str:
    return (
        "import viewprobe._\n"
        "import viewprobe.API._\n"
        "object Work {\n"
        "  val compiled = new Compiled[Int, Int, (Int, Int), "
        "Query[Int, String, Long], List[String]]\n"
        "  val answer: Int = compiled((1, 2)).result\n"
        "  def main(args: Array[String]): Unit = println(answer)\n"
        "}\n"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("native", type=Path)
    parser.add_argument("scala_dist", type=Path)
    parser.add_argument("jdk", type=Path)
    parser.add_argument("--repeats", type=int, default=4)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="view-witness-order-") as scratch:
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
