#!/usr/bin/env python3
"""Compare nested profile type-member resolution with real scalac 2.13.16.

An investigation probe, not a passing regression test. Run after building the
binary whose provenance you want to measure; pass that binary explicitly.
All compiler logs and executed stdout/stderr are retained in a fresh directory.
"""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument("binary", type=Path)
parser.add_argument("--scalac", default="/tmp/scala-2.13.16/bin/scalac")
parser.add_argument("--jar", default="/tmp/scala-rs-lib/scala-library-2.13.16.jar")
args = parser.parse_args()
source = Path(__file__).resolve().parent
root = Path(tempfile.mkdtemp(prefix="returning-family-"))


def command(tag, argv):
    result = subprocess.run([str(a) for a in argv], capture_output=True, timeout=60)
    (root / (tag + ".stdout")).write_bytes(result.stdout)
    (root / (tag + ".stderr")).write_bytes(result.stderr)
    return result


library = root / "library"
library.mkdir()
r = command("library", [args.scalac, source / "Family.scala", "-d", library])
if r.returncode:
    raise SystemExit(f"Oracle library failed; inspect {root}")
rows = []
for case in ("Explicit.scala", "Implicit.scala", "Bad.scala"):
    for mode in ("binary", "source"):
        for compiler in ("nsc", "rs"):
            tag = f"{case}-{mode}-{compiler}"
            out = root / tag
            out.mkdir()
            cmd = [args.scalac] if compiler == "nsc" else [args.binary.resolve(), "compile", "--scala-library", args.jar]
            files = [source / case]
            if mode == "source":
                files.insert(0, source / "Family.scala")
                cp = args.jar
            else:
                cp = f"{library}:{args.jar}"
            compiled = command(tag + "-compile", cmd + files + ["-cp", cp, "-d", out])
            row = {"case": case, "mode": mode, "compiler": compiler, "compile_exit": compiled.returncode}
            if case != "Bad.scala" and compiled.returncode == 0:
                executed = command(tag + "-run", ["java", "-Xverify:all", "-cp", f"{out}:{cp}", "Main"])
                row.update(run_exit=executed.returncode, stdout=executed.stdout.decode(errors="replace"))
            rows.append(row)
comparisons = []
for case in ("Explicit.scala", "Implicit.scala", "Bad.scala"):
    for mode in ("binary", "source"):
        pair = [r for r in rows if r["case"] == case and r["mode"] == mode]
        oracle, candidate = pair
        if (oracle["compile_exit"] == 0) != (case != "Bad.scala"):
            raise SystemExit(f"Unexpected oracle acceptance for {case}/{mode}; inspect {root}")
        comparison = {"case": case, "mode": mode, "acceptance_matches": (oracle["compile_exit"] == 0) == (candidate["compile_exit"] == 0)}
        if "run_exit" in oracle and "run_exit" in candidate:
            comparison["execution_matches"] = oracle["run_exit"] == candidate["run_exit"] == 0 and (root / f"{case}-{mode}-nsc-run.stdout").read_bytes() == (root / f"{case}-{mode}-rs-run.stdout").read_bytes()
        comparisons.append(comparison)
summary = {"comparisons": comparisons, "binary": str(args.binary.resolve()), "logs": str(root), "results": rows}
(root / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps(summary, indent=2))
