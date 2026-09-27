# Standard-library-only typer control

`generate.py` produces a single Scala source with no external dependencies.
Both variants declare the same methods and clients; one uses a wildcard import
in each client, and the control names the member through its object. This
isolates repeated wildcard-import processing while keeping the output shape
nearly identical. No precompiled helper classes are needed.

From the repository root:

```sh
export SCALA_DIST=/path/to/scala-2.13.16
export NATIVE_COMPILER=/path/to/scala-rs
export JDK_DIR=/path/to/jdk
scratch=$(mktemp -d /tmp/wildcard-import.XXXXXX)
library="$SCALA_DIST/lib/scala-library.jar"
compiler_cp="$SCALA_DIST/lib/scala-compiler.jar:$library:$SCALA_DIST/lib/scala-reflect.jar"
python3 tests/perf/wildcard-import/generate.py --members 1024 --uses 100 \
  "$scratch/Wildcard.scala"
python3 tests/perf/wildcard-import/generate.py --members 1024 --uses 100 \
  --qualified "$scratch/Qualified.scala"
"$JDK_DIR/bin/javac" -cp "$compiler_cp" -d "$scratch" \
  tests/perf/process-lifetime/BatchCompile.java
for source in Wildcard Qualified; do
  "$JDK_DIR/bin/java" -Xmx2g -Dscala.usejavacp=true -cp "$scratch:$compiler_cp" \
    BatchCompile "$scratch/$source.scala" "$library" 10 \
    "$NATIVE_COMPILER" "$library"
  "$JDK_DIR/bin/java" -Xmx2g -Dscala.usejavacp=true -cp "$scratch:$compiler_cp" \
    BatchCompile "$scratch/$source.scala" "$library" 10
done
```

Compare the later iterations and, more importantly, each compiler's
`Wildcard - Qualified` difference. The slowdown is in the typer, not parsing
or code generation (`SCALA_RS_PHASE_TIMING=1`). This fixture represents one
specific cost, not every cause of a large multi-project build gap.

After building the native compiler, run the manual regression gate with:

```sh
python3 tests/perf/wildcard-import/check.py "$NATIVE_COMPILER" "$library"
```

It runs both variants in alternating order and fails if the wildcard variant
is at least twice as slow as the qualified control. The earlier linear scan
fails this check; the name-indexed implementation passes it.
