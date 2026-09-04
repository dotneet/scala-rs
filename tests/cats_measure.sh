#!/bin/zsh
# Compile typelevel/cats' kernel + core main sources with scala-rs and report
# the error count. Usage: tests/cats_measure.sh [extra scala-rs args...]
#
# Modelled on tests/slick_measure.sh. Same rules apply:
#   * pin the revision, and re-fetch the material when it is missing, so a
#     reboot that wipes /tmp costs one slow run and not a debugging session;
#   * every path this script writes is per-invocation, so two measurements
#     running at once cannot report each other's numbers;
#   * point CATS_LOG at a path of your own -- the default is shared.
set -e
SP=/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/cats
# v2.13.0, the release whose published jars are in the Coursier cache.
CATS_REV=32a50dcfad9d897459bb755c4b5a22b4c7bc745c
CCACHE=$HOME/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2
if [[ ! -x /tmp/scala-2.13.16/bin/scalac ]]; then
  mkdir -p /tmp/scala-2.13.16/bin /tmp/scala-2.13.16/lib /tmp/scala-rs-lib
  cp $CCACHE/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar /tmp/scala-rs-lib/
  cp $CCACHE/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar /tmp/scala-2.13.16/lib/scala-library.jar
  cp $CCACHE/org/scala-lang/scala-reflect/2.13.16/scala-reflect-2.13.16.jar /tmp/scala-2.13.16/lib/scala-reflect.jar
  cp $CCACHE/org/scala-lang/scala-compiler/2.13.16/scala-compiler-2.13.16.jar /tmp/scala-2.13.16/lib/scala-compiler.jar
  printf '#!/bin/sh\nL=/tmp/scala-2.13.16/lib\nexec java -cp "$L/scala-compiler.jar:$L/scala-library.jar:$L/scala-reflect.jar" scala.tools.nsc.Main "$@"\n' > /tmp/scala-2.13.16/bin/scalac
  chmod +x /tmp/scala-2.13.16/bin/scalac
fi
if [[ ! -d $SP/cats/.git ]]; then
  mkdir -p $SP; rm -rf $SP/cats
  git clone https://github.com/typelevel/cats.git $SP/cats >/dev/null 2>&1
  (cd $SP/cats && git checkout -q $CATS_REV)
fi
# cats generates part of kernel and core with sbt source generators
# (project/KernelBoiler.scala, project/Boilerplate.scala): 1 file for kernel,
# 15 for core. Measuring without them asks for a source set real scalac never
# sees, so let sbt write them once and compile them alongside.
KGEN=$SP/cats/kernel/.jvm/target/scala-2.13/src_managed/main
CGEN=$SP/cats/core/.jvm/target/scala-2.13/src_managed/main
if [[ ! -d $CGEN || ! -d $KGEN ]]; then
  (cd $SP/cats && sbt -batch -Dsbt.supershell=false \
     "kernelJVM/Compile/managedSources" "coreJVM/Compile/managedSources") >/dev/null 2>&1
fi
if [[ ! -s $SP/deps.cp ]]; then
  # What sbt reports for coreJVM/Compile/dependencyClasspath, minus
  # scala-library (passed separately with --scala-library).
  for j in org/typelevel/scalac-compat-annotation_2.13/0.1.4/scalac-compat-annotation_2.13-0.1.4.jar \
           org/scala-lang/scala-reflect/2.13.16/scala-reflect-2.13.16.jar; do
    echo $CCACHE/$j
  done | paste -sd: - > $SP/deps.cp
fi
# ---------------------------------------------------------------------------
# The cross-version source directories sbt selects for 2.13 (`scala`,
# `scala-2`, `scala-2.13+`); `scala-2.12` and `scala-3` are not compiled.
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
if [[ -z ${SCALA_RS:-} ]]; then
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>/tmp/cats_measure_build.log \
    || { cat /tmp/cats_measure_build.log; exit 1; }
fi
# CATS_MODULES picks the source set: `kernel+core` (default, both from source),
# `kernel`, or `core` (core from source against the published cats-kernel jar,
# which is how sbt builds it and the only way to see core's own numbers without
# kernel's in them).
MODULES=${CATS_MODULES:-kernel+core}
DIRS=()
EXTRA_CP=
if [[ $MODULES == *kernel* ]]; then
  DIRS+=($SP/cats/kernel/src/main/scala $SP/cats/kernel/src/main/scala-2.13+ $KGEN)
else
  EXTRA_CP=:$CCACHE/org/typelevel/cats-kernel_2.13/2.13.0/cats-kernel_2.13-2.13.0.jar
fi
if [[ $MODULES == *core* ]]; then
  DIRS+=($SP/cats/core/src/main/scala $SP/cats/core/src/main/scala-2 \
         $SP/cats/core/src/main/scala-2.13+ $CGEN)
fi
# A parse error stops the run before typing, so one unparseable file hides the
# other 339. `core/src/main/scala-2/cats/arrow/FunctionKMacros.scala` matches
# trees with quasiquote *patterns* (`case q"..."`), and interpolated-string
# patterns are not implemented at all -- so it is held out by default and
# counted separately. Set CATS_EXCLUDE='' to measure with it in.
EXCLUDE=${CATS_EXCLUDE-FunctionKMacros.scala}
ALL=($(find $DIRS -name '*.scala' | sort))
if [[ -n $EXCLUDE ]]; then
  FILES=(${ALL:#*$EXCLUDE})
else
  FILES=($ALL)
fi
SKIPPED=$(( ${#ALL[@]} - ${#FILES[@]} ))
RUN=${CATS_RUN:-$SP/run-$$}
OUT=${CATS_OUT:-$RUN/out}
rm -rf $OUT; mkdir -p $OUT
LOG=${CATS_LOG:-$SP/measure.txt}
# cats is built with `-Xsource:3` (sbt-typelevel's default; only the `algebra`
# subproject opts out) and with two compiler plugins, kind-projector 0.13.3 and
# better-monadic-for 0.3.1. We have neither, so every `λ[...]` type lambda in
# the source is ours to handle.
# `-no-specialization` is nsc's own flag ("Ignore @specialize annotations").
# cats writes `import scala.{specialized => sp}` and annotates with `@sp`, which
# we reject without this flag -- and a single parse error aborts the run before
# any file is typechecked, so the count collapses to the parse errors alone and
# says nothing about type checking. Real scalac runs specialization instead; we
# ignore the annotation, which changes the ABI but not what typechecks.
$BIN compile "${FILES[@]}" -d $OUT -cp "$(cat $SP/deps.cp)$EXTRA_CP" -Xsource:3 \
  -no-specialization \
  --scala-library /tmp/scala-rs-lib/scala-library-2.13.16.jar "$@" > $LOG 2>&1 || true
ERRORS=$(grep -c '^error' $LOG || true)
CLASSES=$(find $OUT -name '*.class' | wc -l | tr -d ' ')
# Cascades inflate the raw count; files-with-errors is the honest metric.
BADFILES=$(grep -A 2 '^error' $LOG | grep -oE '(src/main|src_managed/main)/[^:]*' | sort -u | wc -l | tr -d ' ')
rm -rf $RUN
echo "files=${#FILES[@]} skipped=$SKIPPED errors=$ERRORS files_with_errors=$BADFILES classes=$CLASSES"
