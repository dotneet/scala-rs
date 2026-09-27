#!/usr/bin/env python3
"""Manual regression check for wildcard-import scaling."""

import argparse
import statistics
import subprocess
import tempfile
import time
from pathlib import Path

from generate import source


def compile_once(binary: Path, library: Path, input_file: Path, output: Path) -> float:
    output.mkdir()
    started = time.perf_counter()
    result = subprocess.run(
        [str(binary), "compile", str(input_file), "--scala-library", str(library),
         "-d", str(output)],
        capture_output=True,
        text=True,
    )
    elapsed = time.perf_counter() - started
    if result.returncode or not (output / "Main.class").is_file():
        raise RuntimeError(f"compile failed: {result.stdout}\n{result.stderr}")
    return elapsed


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=Path)
    parser.add_argument("scala_library", type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve()
    library = args.scala_library.resolve()
    with tempfile.TemporaryDirectory(prefix="wildcard-regression-") as temp:
        root = Path(temp)
        inputs = {
            "wildcard": root / "Wildcard.scala",
            "qualified": root / "Qualified.scala",
        }
        inputs["wildcard"].write_text(source(1024, 100, True))
        inputs["qualified"].write_text(source(1024, 100, False))
        times = {name: [] for name in inputs}
        for iteration in range(7):
            for name in (("wildcard", "qualified") if iteration % 2 == 0
                         else ("qualified", "wildcard")):
                elapsed = compile_once(
                    binary, library, inputs[name], root / f"out-{iteration}-{name}"
                )
                if iteration:
                    times[name].append(elapsed)
        wildcard = statistics.median(times["wildcard"])
        qualified = statistics.median(times["qualified"])
        ratio = wildcard / qualified
        print(f"wildcard {wildcard:.3f}s qualified {qualified:.3f}s ratio {ratio:.2f}")
        if ratio >= 2.0:
            raise SystemExit("wildcard import scaled more than twice as slowly")


if __name__ == "__main__":
    main()
