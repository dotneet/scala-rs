#!/usr/bin/env python3
"""Compare two compiler binaries on a source manifest, checking their output.

Example (files.txt is one Scala source path per line):
  python3 tests/bench_compare.py /tmp/before target/release/scala-rs \
    --sources-file /tmp/bench/files.txt --scala-library /path/scala-library.jar \
    --classpath "$(cat /tmp/bench/deps.cp)" --compiler-arg=-Xsource:3

Each run uses a fresh output directory. The order alternates by pair; class
bytes, diagnostics and successful exit are checked on every run. CPU time is
child-process user/system time, excluding Python's output hashing and cleanup.
"""

import argparse
import hashlib
import json
from pathlib import Path
import resource
import statistics
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    parser.add_argument("--sources-file", type=Path, required=True)
    parser.add_argument("--scala-library", type=Path, required=True)
    parser.add_argument("--classpath", default="")
    parser.add_argument("--compiler-arg", action="append", default=[])
    parser.add_argument("--reps", type=int, default=4)
    parser.add_argument("--timeout", type=float, default=120)
    args = parser.parse_args()
    if args.reps < 1 or args.timeout <= 0:
        parser.error("--reps and --timeout must be positive")
    files = [line.strip() for line in args.sources_file.read_text().splitlines() if line.strip()]
    if not files or any(not Path(f).is_file() for f in files):
        parser.error("source manifest must contain existing source files")
    results = {"before": [], "after": []}
    reference = None
    for pair in range(args.reps):
        order = ("before", "after") if pair % 2 == 0 else ("after", "before")
        for label in order:
            with tempfile.TemporaryDirectory(prefix="scala-rs-bench-") as tmp:
                out = Path(tmp) / "out"
                cmd = [str(getattr(args, label).resolve()), "compile", *files,
                       "-d", str(out), "--scala-library", str(args.scala_library)]
                if args.classpath:
                    cmd += ["-cp", args.classpath]
                cmd += args.compiler_arg
                cpu = resource.getrusage(resource.RUSAGE_CHILDREN)
                start = time.perf_counter()
                run = subprocess.run(cmd, capture_output=True, timeout=args.timeout)
                wall = time.perf_counter() - start
                end = resource.getrusage(resource.RUSAGE_CHILDREN)
                if run.returncode:
                    raise RuntimeError(
                        f"{label} exited {run.returncode}:\n"
                        + (run.stdout + run.stderr).decode(errors="replace")
                    )
                classes = {
                    str(p.relative_to(out)): hashlib.sha256(p.read_bytes()).hexdigest()
                    for p in out.rglob("*.class")
                }
                if not classes:
                    raise RuntimeError(f"{label} produced no class files")
                diagnostics = tuple(
                    stream.replace(str(out).encode(), b"<output>")
                    for stream in (run.stdout, run.stderr)
                )
                if reference is None:
                    reference = (classes, diagnostics)
                elif reference != (classes, diagnostics):
                    raise RuntimeError(f"{label} class bytes or diagnostics differ from first run")
                row = dict(label=label, pair=pair + 1, wall=wall,
                           user=end.ru_utime - cpu.ru_utime,
                           sys=end.ru_stime - cpu.ru_stime, classes=len(classes))
                results[label].append(row)
                print(json.dumps(row), flush=True)
    medians = {
        label: {key: statistics.median(row[key] for row in rows)
                for key in ("wall", "user", "sys")}
        for label, rows in results.items()
    }
    print(json.dumps(dict(medians=medians, identical_output=True)), flush=True)


if __name__ == "__main__":
    main()
