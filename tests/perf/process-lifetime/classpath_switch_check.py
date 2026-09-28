#!/usr/bin/env python3
"""Compare resident compilers while a standard-library macro classpath alternates."""

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
    parser.add_argument("--repeats", type=int, default=16)
    parser.add_argument("--maximum-ratio", type=float, default=0.8)
    parser.add_argument("--empty-classpath-dirs", type=int, default=0)
    args = parser.parse_args()
    if args.repeats < 2:
        parser.error("--repeats must be at least 2")
    if args.empty_classpath_dirs < 0:
        parser.error("--empty-classpath-dirs must not be negative")

    native = args.native.resolve()
    scala_dist = args.scala_dist.resolve()
    jdk = args.jdk.resolve()
    library = scala_dist / "lib/scala-library.jar"
    reflect = scala_dist / "lib/scala-reflect.jar"
    compiler = scala_dist / "lib/scala-compiler.jar"
    compiler_cp = f"{compiler}:{library}:{reflect}"
    env = dict(os.environ, JAVA_HOME=str(jdk), SCALA_RS_MACRO_DAEMON_REQUIRE="1")

    with tempfile.TemporaryDirectory(prefix="macro-classpath-switch-") as scratch:
        root = Path(scratch)
        providers = [root / "provider-one", root / "provider-two"]
        for provider, name in zip(providers, ("One", "Two")):
            provider.mkdir()
            checked([str(scala_dist / "bin/scalac"), "-nowarn", "-cp",
                     f"{library}:{reflect}", "-d", str(provider),
                     str(HERE / f"ReloadProvider{name}.scala")], env)
        checked([str(jdk / "bin/javac"), "-cp", compiler_cp, "-d", scratch,
                 str(HERE / "BatchCompile.java")], env)
        padding = [root / f"empty-{index:03d}" for index in range(args.empty_classpath_dirs)]
        for directory in padding:
            directory.mkdir()
        suffix = ":".join(str(path) for path in (*padding, library, reflect))
        classpaths = "|".join(f"{provider}:{suffix}" for provider in providers)

        def run(resident: bool) -> float:
            command = [str(jdk / "bin/java"), "-Xmx2g", "-Dscala.usejavacp=true",
                       "-cp", f"{scratch}:{compiler_cp}", "BatchCompile",
                       str(HERE / "ReloadUse.scala"), classpaths, str(args.repeats)]
            if not resident:
                command.extend((str(native), str(library), "--warmup", "--batch-native"))
            else:
                command.append("--warmup")
            lines = checked(command, env).splitlines()
            total = float(next(line.split()[1] for line in lines if line.startswith("total ")))
            outputs = Path(lines[-1])
            for index in (0, args.repeats - 1):
                actual = checked([str(jdk / "bin/java"), "-cp",
                                  f"{outputs / f'out-{index}'}:{providers[index % 2]}:{suffix}",
                                  "probe.ReloadUse"], env)
                expected = f"{index % 2 + 1}\n"
                if actual != expected:
                    raise RuntimeError(f"run {index}: expected {expected!r}, got {actual!r}")
            return total

        ratios = []
        for order in ((False, True), (True, False)):
            times = {mode: run(mode) for mode in order}
            ratio = times[False] / times[True]
            ratios.append(ratio)
            print(f"native {times[False]:.3f}s resident scalac {times[True]:.3f}s "
                  f"ratio {ratio:.2f}")
        if max(ratios) >= args.maximum_ratio:
            raise SystemExit(f"maximum ratio {max(ratios):.2f} is not below "
                             f"{args.maximum_ratio:.2f}")


if __name__ == "__main__":
    main()
