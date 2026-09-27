#!/usr/bin/env python3
"""Reproduce the cost of fresh native processes versus a resident scalac JVM."""

import argparse
import statistics
import subprocess
import tempfile
import time
from pathlib import Path


HERE = Path(__file__).resolve().parent


def checked(command: list[str]) -> str:
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(f"command failed: {command}\n{result.stdout}\n{result.stderr}")
    return result.stdout


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("native", type=Path)
    parser.add_argument("scala_dist", type=Path)
    parser.add_argument("jdk", type=Path)
    parser.add_argument("--repeats", type=int, default=40)
    parser.add_argument("--minimum-ratio", type=float, default=0.0)
    parser.add_argument("--maximum-ratio", type=float)
    args = parser.parse_args()
    if args.repeats < 2:
        parser.error("--repeats must be at least 2")

    native = args.native.resolve()
    scala_dist = args.scala_dist.resolve()
    jdk = args.jdk.resolve()
    library = scala_dist / "lib/scala-library.jar"
    compiler = scala_dist / "lib/scala-compiler.jar"
    reflect = scala_dist / "lib/scala-reflect.jar"
    compiler_cp = ":".join(map(str, (compiler, library, reflect)))
    java = jdk / "bin/java"
    source = HERE / "Tiny.scala"

    with tempfile.TemporaryDirectory(prefix="resident-gap-") as scratch:
        root = Path(scratch)
        checked([str(jdk / "bin/javac"), "-cp", compiler_cp, "-d", scratch,
                 str(HERE / "BatchCompile.java")])

        def run(resident: bool) -> float:
            command = [str(java), "-Xmx2g", "-Dscala.usejavacp=true",
                       "-cp", f"{scratch}:{compiler_cp}", "BatchCompile",
                       str(source), str(library), str(args.repeats)]
            if not resident:
                command.extend((str(native), str(library)))
            lines = checked(command).splitlines()
            total = float(next(line.split()[1] for line in lines if line.startswith("total ")))
            output = Path(lines[-1]) / f"out-{args.repeats - 1}"
            actual = checked([str(java), "-cp", f"{output}:{library}", "Tiny"])
            if actual != "42\n":
                raise RuntimeError(f"unexpected program output: {actual!r}")
            return total

        ratios = []
        for order in ((False, True), (True, False)):
            times = {mode: run(mode) for mode in order}
            ratio = times[False] / times[True]
            ratios.append(ratio)
            print(f"native {times[False]:.3f}s resident scalac {times[True]:.3f}s "
                  f"ratio {ratio:.2f}")

        fresh = []
        for index in range(3):
            output = root / f"fresh-{index}"
            output.mkdir()
            started = time.perf_counter()
            checked([str(scala_dist / "bin/scalac"), "-nowarn", "-cp", str(library),
                     "-d", str(output), str(source)])
            fresh.append(time.perf_counter() - started)
        print(f"fresh scalac median {statistics.median(fresh):.3f}s per compile")
        if min(ratios) < args.minimum_ratio:
            raise SystemExit(f"minimum ratio {min(ratios):.2f} is below "
                             f"{args.minimum_ratio:.2f}")
        if args.maximum_ratio is not None and max(ratios) >= args.maximum_ratio:
            raise SystemExit(f"maximum ratio {max(ratios):.2f} is not below "
                             f"{args.maximum_ratio:.2f}")


if __name__ == "__main__":
    main()
