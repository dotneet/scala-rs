#!/usr/bin/env python3
"""Check that a warm macro engine does not retain another run's source classes."""

import argparse
import hashlib
import os
import subprocess
import tempfile
from pathlib import Path


HERE = Path(__file__).resolve().parent


def run(command: list[str], env: dict[str, str]) -> None:
    result = subprocess.run(command, env=env, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(f"command failed: {command}\n{result.stdout}\n{result.stderr}")


def hashes(root: Path) -> dict[str, str]:
    return {str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in root.rglob("*.class")}


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
    reflect = scala_dist / "lib/scala-reflect.jar"
    java = jdk / "bin/java"
    env = dict(os.environ, JAVA_HOME=str(jdk))

    with tempfile.TemporaryDirectory(prefix="macro-mirror-reload-") as scratch:
        root = Path(scratch)
        provider = root / "provider"
        provider.mkdir()
        run([str(scala_dist / "bin/scalac"), "-nowarn", "-cp", f"{library}:{reflect}",
             "-d", str(provider), str(HERE / "MirrorProvider.scala")], env)
        base_cp = f"{provider}:{library}:{reflect}"

        def compile_sequence(compiler: str) -> None:
            out = root / compiler
            out.mkdir()
            outputs = [out / name for name in ("a1", "b", "a2", "run")]
            for directory in outputs:
                directory.mkdir()
            sources = ["MirrorA.scala", "MirrorB.scala", "MirrorA.scala", "MirrorRun.scala"]
            classpaths = [base_cp, f"{outputs[0]}:{base_cp}", base_cp,
                          f"{outputs[0]}:{outputs[1]}:{base_cp}"]
            for source, classpath, directory in zip(sources, classpaths, outputs):
                if compiler == "native":
                    command = [str(native), "compile", str(HERE / source), "-cp", classpath,
                               "--scala-library", str(library), "-nowarn", "-d", str(directory)]
                else:
                    command = [str(scala_dist / "bin/scalac"), "-nowarn", "-cp", classpath,
                               "-d", str(directory), str(HERE / source)]
                run(command, dict(env, SCALA_RS_MACRO_DAEMON="1",
                                  SCALA_RS_MACRO_DAEMON_REQUIRE="1"))
            if hashes(outputs[0]) != hashes(outputs[2]):
                raise RuntimeError(f"{compiler}: first and second A class files differ")
            result = subprocess.run([str(java), "-cp",
                                     f"{outputs[3]}:{outputs[1]}:{outputs[0]}:{base_cp}",
                                     "probe.MirrorRun"], env=env, capture_output=True, text=True)
            if result.returncode or result.stdout != "3\n":
                raise RuntimeError(f"{compiler}: unexpected runtime output {result.stdout!r} "
                                   f"{result.stderr!r}")
            print(f"{compiler}: A-B-A class hashes and program output passed")

        compile_sequence("scalac")
        compile_sequence("native")


if __name__ == "__main__":
    main()
