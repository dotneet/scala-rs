# Compiler process-lifetime reproduction

This source uses only the Scala standard library. Compile the same source 100
times to distinct input paths and fresh output directories. The native compiler
starts once per compilation; the reference compiler creates a new compiler
instance per compilation inside one JVM. The first reference compilation
includes compiler warm-up; JVM launch is outside the measured interval.

Run from this directory with Scala 2.13.16 and a JDK installed:

```sh
export SCALA_DIST=/path/to/scala-2.13.16
export NATIVE_COMPILER=/path/to/scala-rs
export JDK_DIR=/path/to/jdk
scratch=$(mktemp -d /tmp/compiler-batch.XXXXXX)
library="$SCALA_DIST/lib/scala-library.jar"
compiler_cp="$SCALA_DIST/lib/scala-compiler.jar:$library:$SCALA_DIST/lib/scala-reflect.jar"
"$JDK_DIR/bin/javac" -cp "$compiler_cp" -d "$scratch" BatchCompile.java
"$JDK_DIR/bin/java" -Xmx2g -Dscala.usejavacp=true -cp "$scratch:$compiler_cp" \
  BatchCompile Tiny.scala "$library" 100 "$NATIVE_COMPILER" "$library"
"$JDK_DIR/bin/java" -Xmx2g -Dscala.usejavacp=true -cp "$scratch:$compiler_cp" \
  BatchCompile Tiny.scala "$library" 100
```

Compare the `total` lines. Both modes create new output directories, and the
source copies have identical bytes but distinct paths. Compile and run `Tiny`
once with each compiler to check the expected output, `42`.

This isolates repeated compiler setup and JVM reuse. It does not establish how
much of a larger build's gap comes from that mechanism; classpath completion,
implicit search, macros, and other work are absent from this fixture.
