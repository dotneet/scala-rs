#!/usr/bin/env python3
"""Require the resident compiler to beat resident scalac on empty subclasses."""

import argparse
import statistics
import subprocess
import tempfile
from pathlib import Path

from generate import write_fixture


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
REPEATS = 20


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
    args = parser.parse_args()
    native = args.native.resolve()
    scala_dist = args.scala_dist.resolve()
    jdk = args.jdk.resolve()
    library = scala_dist / "lib/scala-library.jar"
    compiler_cp = ":".join(
        str(scala_dist / f"lib/{name}.jar")
        for name in ("scala-compiler", "scala-library", "scala-reflect")
    )
    java = jdk / "bin/java"

    with tempfile.TemporaryDirectory(prefix="inherited-headers-") as scratch:
        root = Path(scratch)
        source = root / "InheritedHeaders.scala"
        write_fixture(source)
        checked(
            [str(jdk / "bin/javac"), "-cp", compiler_cp, "-d", scratch,
             str(ROOT / "tests/perf/process-lifetime/BatchCompile.java")]
        )

        def run(resident_native: bool) -> float:
            command = [
                str(java), "-Xmx2g", "-Dscala.usejavacp=true", "-cp",
                f"{scratch}:{compiler_cp}", "BatchCompile", str(source),
                str(library), str(REPEATS),
            ]
            if resident_native:
                command.extend((str(native), str(library), "--batch-native"))
            command.append("--warmup")
            lines = checked(command).splitlines()
            times = [float(line.split()[1]) for line in lines if line[0].isdigit()]
            if len(times) != REPEATS:
                raise RuntimeError(f"expected {REPEATS} timings: {lines}")
            output = Path(lines[-1]) / f"out-{REPEATS - 1}"
            actual = checked([str(java), "-cp", f"{output}:{library}",
                              "inheritedheaders.InheritedHeaders"])
            if actual != "1\n":
                raise RuntimeError(f"unexpected program output: {actual!r}")
            return statistics.median(times[10:])

        ratios = []
        for order in ((True, False), (False, True)):
            times = {mode: run(mode) for mode in order}
            ratio = times[True] / times[False]
            ratios.append(ratio)
            print(f"native {times[True]:.3f}s resident scalac {times[False]:.3f}s "
                  f"ratio {ratio:.2f}")
        if max(ratios) >= 1.0:
            raise SystemExit(f"maximum ratio {max(ratios):.2f} is not below 1.00")


if __name__ == "__main__":
    main()
