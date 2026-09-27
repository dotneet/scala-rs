# Macro round-trip reproduction

This fixture has no third-party dependencies. `scala-reflect.jar` is required
because it defines the Scala 2 macro API; use the jar from the same Scala
2.13.16 distribution as `scalac`. `ProbeMacro.scala` compiles once, before the
timed client compilations. Both clients expand the same macro 16 times and
produce `672`. Only `ManyQueries.scala` asks the compiler to parse and typecheck
400 expressions per expansion. Thus the difference between the clients
isolates 6,400 macro-to-compiler queries from macro startup and call-site work.

From the repository root:

```sh
export SCALA_DIST=/path/to/scala-2.13.16
export NATIVE_COMPILER=/path/to/scala-rs
export JDK_DIR=/path/to/jdk
scratch=$(mktemp -d /tmp/macro-roundtrips.XXXXXX)
library="$SCALA_DIST/lib/scala-library.jar"
reflect="$SCALA_DIST/lib/scala-reflect.jar"
compiler_cp="$SCALA_DIST/lib/scala-compiler.jar:$library:$reflect"
"$SCALA_DIST/bin/scalac" -cp "$library:$reflect" -d "$scratch" \
  tests/perf/macro-roundtrips/ProbeMacro.scala
"$JDK_DIR/bin/javac" -cp "$compiler_cp" -d "$scratch" \
  tests/perf/process-lifetime/BatchCompile.java
client_cp="$library:$reflect:$scratch"
for source in NoQueries ManyQueries; do
  "$JDK_DIR/bin/java" -Xmx2g -Dscala.usejavacp=true -cp "$scratch:$compiler_cp" \
    BatchCompile "tests/perf/macro-roundtrips/$source.scala" "$client_cp" 8 \
    "$NATIVE_COMPILER" "$library"
  "$JDK_DIR/bin/java" -Xmx2g -Dscala.usejavacp=true -cp "$scratch:$compiler_cp" \
    BatchCompile "tests/perf/macro-roundtrips/$source.scala" "$client_cp" 8
done
```

Compare the later iterations rather than the first scalac iteration, which
includes JVM warm-up. To locate the native work, repeat one compilation with
`SCALA_RS_PHASE_TIMING=1 SCALA_RS_MACRO_TIMING=1`.
