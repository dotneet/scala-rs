#!/usr/bin/env python3
"""Compile unchanged refined coreJVM sources selected by sbt, then run JVM probes.

See docs/refined.md for preparing the pinned checkout and the sbt source listing.
The output directory must be new; official refined classes never enter the probes.
"""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import time

REFINED_COMMIT = "11560e094e8cb4ea5b6c09a9f72572f179cc80b9"
REPO = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checkout", required=True, type=Path)
    parser.add_argument("--sbt-log", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--compiler", type=Path, default=REPO / "target/release/scala-rs")
    args = parser.parse_args()
    checkout = args.checkout.resolve()

    def git(*options):
        return subprocess.check_output(["git", "-C", str(checkout), *options], text=True).strip()

    if git("rev-parse", "HEAD") != REFINED_COMMIT or git("status", "--porcelain"):
        parser.error("refined must be an unchanged checkout of " + REFINED_COMMIT)
    log = args.sbt_log.read_text()
    sources = re.findall(r"^\[info\] \* (/.+\.scala)$", log, re.M)
    deps = re.findall(r"^\[info\] \* Attributed\((.+)\)$", log, re.M)
    if len(set(sources)) != 45 or len(sources) != 45 or len(deps) != 6:
        parser.error("expected the 45 coreJVM sources and 6 dependency jars from sbt")
    if not all(Path(p).is_file() for p in sources + deps):
        parser.error("a source or dependency listed by sbt no longer exists")
    if not all(Path(p).resolve().is_relative_to(checkout) for p in sources):
        parser.error("sbt listed sources outside the refined checkout")
    library = next(p for p in deps if Path(p).name == "scala-library-2.13.18.jar")
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    compiler = str(args.compiler.resolve())
    status = {"refined_commit": REFINED_COMMIT, "source_count": len(sources), "steps": []}
    (out / "sources.json").write_text(json.dumps(sources, indent=2) + "\n")
    (out / "classpath.json").write_text(json.dumps(deps, indent=2) + "\n")

    def run(name, command):
        step = {"name": name, "command": command}
        status["steps"].append(step)
        start = time.monotonic()
        try:
            with (out / (name + ".log")).open("w") as stream:
                result = subprocess.run(command, stdout=stream, stderr=subprocess.STDOUT, timeout=180)
            step["exit"] = result.returncode
            result.check_returncode()
        finally:
            step["seconds"] = round(time.monotonic() - start, 3)
            (out / "result.json").write_text(json.dumps(status, indent=2) + "\n")

    classes = out / "classes"
    classes.mkdir()
    run("refined-compile", [compiler, "compile", *sources, "--scala-library", library,
        "-cp", os.pathsep.join(deps), "-d", str(classes), "--diagnostics=scalac",
        "-feature", "-unchecked", "-language:existentials,experimental.macros,higherKinds,implicitConversions",
        "-Xfatal-warnings"])
    status["class_count"] = len(list(classes.rglob("*.class")))
    artifact = out / "refined_2.13-scala-rs.jar"
    run("refined-jar", ["jar", "cf", str(artifact), "-C", str(classes), "."])
    status["artifact"] = str(artifact)
    cp = os.pathsep.join([str(artifact), *deps])
    smoke = str(REPO / "tests/refinedrun/Smoke.scala")
    for producer in ["scalac", "scala-rs"]:
        consumer = out / producer
        consumer.mkdir()
        command = (["java", "-cp", os.pathsep.join(deps), "scala.tools.nsc.Main"]
            if producer == "scalac" else [compiler, "compile", "--scala-library", library])
        run(producer + "-compile", [*command, smoke, "-cp", cp, "-d", str(consumer)])
        run(producer + "-run", ["java", "-Xverify:all", "-cp",
            os.pathsep.join([str(consumer), cp]), "RefinedSmoke"])
        if (out / (producer + "-run.log")).read_text().strip() != "REFINED_SMOKE_PASS":
            raise RuntimeError("missing runtime completion marker: " + producer)
    status["verdict"] = "PASS"
    (out / "result.json").write_text(json.dumps(status, indent=2) + "\n")
    print(json.dumps({k: v for k, v in status.items() if k != "steps"}))


if __name__ == "__main__":
    main()
