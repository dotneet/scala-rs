#!/usr/bin/env python3
"""Check that a resident macro engine honors the current provider classpath."""

import argparse
import hashlib
import os
import shutil
import subprocess
import tempfile
from pathlib import Path


HERE = Path(__file__).resolve().parent


def run(command: list[str], env: dict[str, str]) -> str:
    result = subprocess.run(command, env=env, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError(f"command failed: {command}\n{result.stdout}\n{result.stderr}")
    return result.stdout


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
    env = dict(os.environ, JAVA_HOME=str(jdk), SCALA_RS_MACRO_DAEMON="1",
               SCALA_RS_MACRO_DAEMON_REQUIRE="1")

    with tempfile.TemporaryDirectory(prefix="macro-provider-reload-") as scratch:
        root = Path(scratch)
        providers = [root / "provider-one", root / "provider-two"]
        for provider, name in zip(providers, ("One", "Two")):
            provider.mkdir()
            run([str(scala_dist / "bin/scalac"), "-nowarn", "-cp",
                 f"{library}:{reflect}", "-d", str(provider),
                 str(HERE / f"ReloadProvider{name}.scala")], env)
        implementation = Path("probe/ReloadProviderImpl$.class")
        if ((providers[0] / implementation).stat().st_size !=
                (providers[1] / implementation).stat().st_size):
            raise RuntimeError("provider implementation sizes must match")

        for compiler in ("scalac", "native"):
            for strategy in ("separate", "in-place"):
                previous = None
                live = root / f"{compiler}-live"
                original_times = {}
                for index, provider in enumerate((providers[0], providers[1], providers[0])):
                    if strategy == "in-place":
                        shutil.copytree(provider, live, dirs_exist_ok=True)
                        for path in live.rglob("*.class"):
                            relative = path.relative_to(live)
                            if index == 0:
                                stat = path.stat()
                                original_times[relative] = (stat.st_atime_ns, stat.st_mtime_ns)
                            else:
                                os.utime(path, ns=original_times[relative])
                        provider = live
                    output = root / f"{compiler}-{strategy}-{index}"
                    output.mkdir()
                    classpath = f"{provider}:{library}:{reflect}"
                    if compiler == "scalac":
                        command = [str(scala_dist / "bin/scalac"), "-nowarn", "-cp", classpath,
                                   "-d", str(output), str(HERE / "ReloadUse.scala")]
                    else:
                        command = [str(native), "compile", str(HERE / "ReloadUse.scala"),
                                   "-cp", classpath, "--scala-library", str(library),
                                   "-nowarn", "-d", str(output)]
                    run(command, env)
                    actual = run([str(jdk / "bin/java"), "-cp", f"{output}:{classpath}",
                                  "probe.ReloadUse"], env)
                    expected = ("1\n", "2\n", "1\n")[index]
                    if actual != expected:
                        raise RuntimeError(f"{compiler} {strategy} run {index}: expected "
                                           f"{expected!r}, got {actual!r}")
                    if index == 0:
                        previous = hashes(output)
                    elif index == 2 and previous != hashes(output):
                        raise RuntimeError(f"{compiler} {strategy}: first and third output "
                                           "hashes differ")
                print(f"{compiler}: {strategy} provider A-B-A passed")


if __name__ == "__main__":
    main()
