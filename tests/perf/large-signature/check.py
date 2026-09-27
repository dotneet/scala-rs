#!/usr/bin/env python3
"""Regression check for importing a large pickled member signature."""

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def provider_source(members: int) -> str:
    lines = [
        "package signatureprobe",
        "trait Decoder[F[_], A] { type Out; def apply(value: F[A]): Out }",
        "trait Identity[F[_], A, B]",
        "final class Ops[F[_], A, B](val value: F[A])",
        "trait Catalog {",
    ]
    lines.extend(
        f"  implicit def lift{i}[F[_], A <: Product with Serializable, "
        f"B >: A <: AnyRef](value: F[A])(implicit decoder: Decoder[F, A], "
        f"evidence: A <:< B, identity: Identity[F, A, B]): Ops[F, A, B] "
        f"= new Ops[F, A, B](value)"
        for i in range(members)
    )
    lines.extend(("}", "object All extends Catalog"))
    return "\n".join(lines) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("native", type=Path)
    parser.add_argument("scala_dist", type=Path)
    parser.add_argument("jdk", type=Path)
    parser.add_argument("--members", type=int, default=1000)
    parser.add_argument("--repeats", type=int, default=20)
    parser.add_argument("--maximum-ratio", type=float, default=2.0)
    args = parser.parse_args()
    if args.members < 128:
        parser.error("--members must be at least 128")
    with tempfile.TemporaryDirectory(prefix="large-signature-") as scratch:
        provider = Path(scratch) / "Provider.scala"
        source = Path(scratch) / "Use.scala"
        provider.write_text(provider_source(args.members))
        source.write_text(
            "import signatureprobe.All._\n"
            "object Use { def main(args: Array[String]): Unit = println(1) }\n"
        )
        subprocess.run(
            [
                sys.executable,
                str(ROOT / "perf/process-lifetime/macro_check.py"),
                str(args.native),
                str(args.scala_dist),
                str(args.jdk),
                "--provider",
                str(provider),
                "--source",
                str(source),
                "--main-class",
                "Use",
                "--expected-output",
                "1",
                "--resident-compiler",
                "--daemon-policy",
                "default",
                "--repeats",
                str(args.repeats),
                "--maximum-ratio",
                str(args.maximum_ratio),
            ],
            check=True,
        )


if __name__ == "__main__":
    main()
