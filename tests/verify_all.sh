#!/usr/bin/env bash
# Load every emitted class with verification on, and report the ones the JVM
# refuses.
#
# This exists because the rest of the battery could not see them. On 2026-09-06
# six of the 1490 classes slick compiles to failed `VerifyError` on `main`, and
# had been failing for an unknown number of waves:
#
#   * `slick_subset.sh` calls `Class.forName(name, false, loader)`. The `false`
#     means *do not initialise*, and a class that is not initialised is not
#     linked either, so its method bodies are never verified.
#   * `slick_run.sh` runs twelve programs. It verifies whatever those programs
#     touch, which is most of what matters and is not all of it.
#   * `classfile_lint.py` reads structure -- branch targets, method sizes. It
#     does not type anything.
#   * a `javap -p` sweep stops at the constant pool.
#
# So this is the check that was missing: `Class.forName(name, true, loader)`
# over every class file in a directory, counting `VerifyError` and
# `ClassFormatError`. Anything else a load can throw -- a missing dependency, a
# static initialiser that wants a database -- is not this script's business and
# is ignored.
#
#   tests/verify_all.sh <classes-dir> [extra classpath entries...]
#
# Exits non-zero when any class fails verification.
set -uo pipefail

DIR=${1:?usage: verify_all.sh <classes-dir> [cp...]}
shift
LIB=${SCALA_LIBRARY_JAR:-/tmp/scala-rs-lib/scala-library-2.13.16.jar}
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

CP="$LIB"
for e in "$@"; do CP="$CP:$e"; done

cat > "$WORK/VerifyAll.java" <<'JAVA'
import java.io.*; import java.nio.file.*; import java.net.*; import java.util.*;

public class VerifyAll {
  public static void main(String[] a) throws Exception {
    Path root = Paths.get(a[0]);
    List<String> names = new ArrayList<>();
    Files.walk(root).filter(p -> p.toString().endsWith(".class")).forEach(p -> {
      String n = root.relativize(p).toString();
      names.add(n.substring(0, n.length() - 6).replace(File.separatorChar, '.'));
    });
    Collections.sort(names);

    List<URL> cp = new ArrayList<>();
    cp.add(root.toUri().toURL());
    for (String e : a[1].split(File.pathSeparator))
      if (!e.isEmpty()) cp.add(new File(e).toURI().toURL());
    URLClassLoader cl = new URLClassLoader(cp.toArray(new URL[0]), null);

    int bad = 0;
    for (String n : names) {
      try {
        // `true` is the whole point: initialising forces linking, and linking
        // is what runs the verifier over the method bodies.
        Class.forName(n, true, cl);
      } catch (VerifyError | ClassFormatError e) {
        bad++;
        String m = String.valueOf(e.getMessage()).split("\n")[0];
        System.out.println("BAD " + n + " :: " + e.getClass().getSimpleName() + ": " + m);
      } catch (Throwable t) {
        // A missing dependency or a static initialiser with opinions is not
        // what this script is asking about.
      }
    }
    System.out.println("verify_classes=" + names.size() + " verify_failures=" + bad);
    if (bad > 0) System.exit(1);
  }
}
JAVA

javac -d "$WORK" "$WORK/VerifyAll.java" >/dev/null 2>&1 || {
  echo "verify_all: javac failed" >&2
  exit 2
}
java -cp "$WORK" VerifyAll "$DIR" "$CP"
