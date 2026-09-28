#!/usr/bin/env python3
"""Check a macro mirror after a resident compiler gains a classpath directory."""

import argparse
import os
from pathlib import Path
import subprocess
import tempfile


HERE = Path(__file__).resolve().parent


def checked(command: list[str], env: dict[str, str]) -> str:
    result = subprocess.run(command, env=env, capture_output=True, text=True)
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
    reflect = scala_dist / "lib/scala-reflect.jar"
    compiler = scala_dist / "lib/scala-compiler.jar"
    compiler_cp = f"{compiler}:{library}:{reflect}"
    env = dict(os.environ, JAVA_HOME=str(jdk), SCALA_RS_MACRO_DAEMON_REQUIRE="1")

    with tempfile.TemporaryDirectory(prefix="macro-classpath-addition-") as scratch:
        root = Path(scratch)
        provider = root / "provider"
        added = root / "added"
        provider.mkdir()
        added.mkdir()
        for source, output in (("ClasspathProbe.scala", provider),
                               ("ClasspathAdded.scala", added)):
            checked([str(scala_dist / "bin/scalac"), "-nowarn", "-cp",
                     f"{library}:{reflect}", "-d", str(output), str(HERE / source)], env)
        checked([str(jdk / "bin/javac"), "-cp", compiler_cp, "-d", scratch,
                 str(HERE / "BatchCompile.java")], env)
        before = f"{provider}:{library}:{reflect}"
        after = f"{provider}:{added}:{library}:{reflect}"
        for compiler_name in ("scalac", "native"):
            command = [str(jdk / "bin/java"), "-Xmx2g", "-Dscala.usejavacp=true",
                       "-cp", f"{scratch}:{compiler_cp}", "BatchCompile",
                       str(HERE / "ClasspathUse.scala"), f"{before}|{after}", "2"]
            if compiler_name == "native":
                command.extend((str(native), str(library), "--batch-native"))
            lines = checked(command, env).splitlines()
            outputs = Path(lines[-1])
            for index, classpath, expected in ((0, before, "false\n"),
                                               (1, after, "true\n")):
                actual = checked([str(jdk / "bin/java"), "-cp",
                                  f"{outputs / f'out-{index}'}:{classpath}",
                                  "probe.ClasspathUse"], env)
                if actual != expected:
                    raise RuntimeError(f"{compiler_name} run {index}: "
                                       f"expected {expected!r}, got {actual!r}")
            print(f"{compiler_name}: absent then added class passed")


if __name__ == "__main__":
    main()
