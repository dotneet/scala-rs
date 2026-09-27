#!/usr/bin/env python3
"""Compare a fresh native compile with a new scalac instance in a resident JVM."""

import argparse
import os
import subprocess
import tempfile
from pathlib import Path


HERE = Path(__file__).resolve().parent


def checked(command: list[str], env: dict[str, str] | None = None) -> str:
    result = subprocess.run(command, capture_output=True, text=True, env=env)
    if result.returncode:
        raise RuntimeError(f"command failed: {command}\n{result.stdout}\n{result.stderr}")
    return result.stdout


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("native", type=Path)
    parser.add_argument("scala_dist", type=Path)
    parser.add_argument("jdk", type=Path)
    parser.add_argument("--repeats", type=int, default=20)
    parser.add_argument("--resident-native", action="store_true",
                        help="require the macro daemon and warm both resident compilers once")
    parser.add_argument("--resident-compiler", action="store_true",
                        help="also keep the native compiler process alive between repetitions")
    parser.add_argument("--daemon-policy", choices=("required", "default", "disabled"),
                        default="required", help="macro daemon setting for resident native runs")
    parser.add_argument("--maximum-ratio", type=float)
    parser.add_argument("--minimum-ratio", type=float)
    parser.add_argument("--empty-classpath-dirs", type=int, default=0)
    parser.add_argument("--provider", type=Path, default=HERE / "Marker.scala")
    parser.add_argument("--source", type=Path, default=HERE / "MacroUse.scala")
    parser.add_argument("--main-class", default="MacroUse")
    parser.add_argument("--expected-output", default="2")
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
    compiler_cp = ":".join(map(str, (compiler, library, reflect)))
    java = jdk / "bin/java"
    native_env = None
    if args.resident_native or args.resident_compiler:
        native_env = dict(os.environ)
        if args.daemon_policy == "required":
            native_env["SCALA_RS_MACRO_DAEMON"] = "1"
            native_env["SCALA_RS_MACRO_DAEMON_REQUIRE"] = "1"
        elif args.daemon_policy == "default":
            native_env.pop("SCALA_RS_MACRO_DAEMON", None)
            native_env["SCALA_RS_MACRO_DAEMON_REQUIRE"] = "1"
        else:
            native_env["SCALA_RS_MACRO_DAEMON"] = "0"
            native_env.pop("SCALA_RS_MACRO_DAEMON_REQUIRE", None)

    with tempfile.TemporaryDirectory(prefix="macro-lifetime-") as scratch:
        root = Path(scratch)
        provider = root / "provider"
        provider.mkdir()
        checked([str(scala_dist / "bin/scalac"), "-nowarn", "-cp",
                 f"{library}:{reflect}", "-d", str(provider), str(args.provider.resolve())])
        checked([str(jdk / "bin/javac"), "-cp", compiler_cp, "-d", scratch,
                 str(HERE / "BatchCompile.java")])
        padding = [root / f"empty-{index:03d}" for index in range(args.empty_classpath_dirs)]
        for directory in padding:
            directory.mkdir()
        classpath = ":".join(map(str, (provider, *padding, library, reflect)))

        def run(resident: bool) -> float:
            command = [str(java), "-Xmx2g", "-Dscala.usejavacp=true",
                       "-cp", f"{scratch}:{compiler_cp}", "BatchCompile",
                       str(args.source.resolve()), classpath, str(args.repeats)]
            if not resident:
                command.extend((str(native), str(library)))
            if args.resident_compiler and not resident:
                command.append("--batch-native")
            if args.resident_native or args.resident_compiler:
                command.append("--warmup")
            lines = checked(command, env=native_env if not resident else None).splitlines()
            total = float(next(line.split()[1] for line in lines if line.startswith("total ")))
            output = Path(lines[-1]) / f"out-{args.repeats - 1}"
            actual = checked([str(java), "-cp", f"{output}:{classpath}", args.main_class])
            if actual != args.expected_output + "\n":
                raise RuntimeError(f"unexpected program output: {actual!r}")
            return total

        ratios = []
        for order in ((False, True), (True, False)):
            times = {mode: run(mode) for mode in order}
            ratio = times[False] / times[True]
            ratios.append(ratio)
            print(f"native {times[False]:.3f}s resident scalac {times[True]:.3f}s "
                  f"ratio {ratio:.2f}")
        if args.maximum_ratio is not None and max(ratios) >= args.maximum_ratio:
            raise SystemExit(f"maximum ratio {max(ratios):.2f} is not below "
                             f"{args.maximum_ratio:.2f}")
        if args.minimum_ratio is not None and min(ratios) <= args.minimum_ratio:
            raise SystemExit(f"minimum ratio {min(ratios):.2f} is not above "
                             f"{args.minimum_ratio:.2f}")


if __name__ == "__main__":
    main()
