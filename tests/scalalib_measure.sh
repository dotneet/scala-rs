#!/bin/zsh
# Compile scala/scala's own standard library (`src/library`) with scala-rs and
# report the error count. Usage: tests/scalalib_measure.sh [extra scala-rs args...]
#
# Modelled on tests/slick_measure.sh and tests/cats_measure.sh. Same rules:
#   * pin the revision and re-fetch when the material is missing, so a reboot
#     that wipes the scratchpad costs one slow run, not a debugging session;
#   * every path this script writes is per-invocation, so two measurements
#     running at once cannot report each other's numbers;
#   * point SCALALIB_LOG at a path of your own -- the default is shared.
#
# Why the classpath is *not* scala-library-2.13.16.jar: the sources under
# measurement define the very classes that jar contains, so linking against it
# reports a duplicate for every definition in the library (`<overload None$ |
# None$>`) and measures nothing. The point of this measurement is the opposite:
# can the library be built *without* a prebuilt scala-library? So we run in
# `--no-scala-library` mode and put on the classpath only the 33 classfiles in
# the jar that come from the library's 32 *Java* sources (BoxesRunTime,
# Statics, the *Ref boxes, BoxedUnit, ScalaNumber, the concurrent TrieMap
# bases, ScalaSignature). Real scalac gets those from javac in the same run;
# we have no Java front end, so they come from the jar. `SCALALIB_MODE=jar`
# measures the other arrangement (full jar, `--scala-library`) for comparison.
set -e
SP=/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/scalalib
# v2.13.16 -- the same release as the jar the rest of the test suite links
# against, so the sources and the Java classfiles below are from one tree.
SCALA_REV=3f6bdaeafde17d790023cc3f299b81eaaf876ca3
CCACHE=$HOME/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2
if [[ ! -f /tmp/scala-rs-lib/scala-library-2.13.16.jar ]]; then
  mkdir -p /tmp/scala-rs-lib
  cp $CCACHE/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar /tmp/scala-rs-lib/
fi
if [[ ! -d $SP/scala/.git ]]; then
  mkdir -p $SP; rm -rf $SP/scala
  git clone --depth 1 --branch v2.13.16 https://github.com/scala/scala.git $SP/scala >/dev/null 2>&1
fi
(cd $SP/scala && [[ $(git rev-parse HEAD) == $SCALA_REV ]]) \
  || { echo "scala checkout is not $SCALA_REV" >&2; exit 1; }
# The Java half of the library, taken from the released jar. Rebuilt whenever
# it is missing; the list is exactly `find src/library -name '*.java'`.
JAVACP=$SP/javacp/keep
if [[ ! -d $JAVACP ]]; then
  rm -rf $SP/javacp; mkdir -p $SP/javacp/all $JAVACP
  unzip -q /tmp/scala-rs-lib/scala-library-2.13.16.jar -d $SP/javacp/all
  for c in $(cd $SP/scala/src/library && find . -name '*.java' | sed 's,^\./,,;s,\.java$,,'); do
    mkdir -p $JAVACP/$(dirname $c)
    cp $SP/javacp/all/$c*.class $JAVACP/$(dirname $c)/ 2>/dev/null || true
  done
  rm -rf $SP/javacp/all
fi
# ---------------------------------------------------------------------------
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
# The release binary is not what `cargo test` builds; measuring a stale one
# silently reports the previous commit's numbers.
if [[ -z ${SCALA_RS:-} ]]; then
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>/tmp/scalalib_measure_build.log \
    || { cat /tmp/scalalib_measure_build.log; exit 1; }
fi
# SCALALIB_DIRS picks the source set; `src/library` is the whole point, and the
# other two (`src/reflect`, `src/compiler`) are there for when it stops being
# the bottleneck. `src/library-aux` is never compiled -- Any/AnyRef/Nothing/
# Null/Singleton are scaladoc stubs for symbols the compiler defines itself
# (build.sbt passes them as `-doc-no-compile`).
DIRS=(${=SCALALIB_DIRS:-$SP/scala/src/library})
FILES=($(find $DIRS -name '*.scala' | sort))
RUN=${SCALALIB_RUN:-$SP/run-$$}
OUT=${SCALALIB_OUT:-$RUN/out}
rm -rf $OUT; mkdir -p $OUT
LOG=${SCALALIB_LOG:-$SP/measure.txt}
# Flags: build.sbt gives the library `-feature -Xlint -Wconf:... -sourcepath
# <scalaSource>` and, in CI only, `-Werror`. None of them changes what is
# accepted, so there is nothing to pass on: no -Xsource:3, no -Yrecursion, no
# -opt (the optimiser is only turned on for the bootstrap and the benchmarks).
COMPILER_EXIT=0
if [[ ${SCALALIB_MODE:-nolib} == jar ]]; then
  $BIN compile "${FILES[@]}" -d $OUT -no-specialization \
    --scala-library /tmp/scala-rs-lib/scala-library-2.13.16.jar "$@" > $LOG 2>&1 || COMPILER_EXIT=$?
else
  $BIN compile "${FILES[@]}" -d $OUT -cp $JAVACP -no-specialization --no-scala-library "$@" > $LOG 2>&1 || COMPILER_EXIT=$?
fi
# `-no-specialization` is nsc's own flag. The library annotates with
# `@specialized` everywhere, we reject that annotation without the flag, and a
# single parse error aborts the run before any file is typechecked -- so the
# count collapses to the parse errors alone (84) and says nothing about type
# checking. Same trap as tests/cats_measure.sh; see docs/scala-library.md.
ERRORS=$(grep -c '^error' $LOG || true)
CLASSES=$(find $OUT -name '*.class' | wc -l | tr -d ' ')
# Cascades inflate the raw count; files-with-errors is the honest metric.
BADFILES=$(grep -A 2 '^error' $LOG | grep -oE 'src/(library|reflect|compiler)/[^:]*' | sort -u | wc -l | tr -d ' ')
rm -rf $RUN
echo "files=${#FILES[@]} errors=$ERRORS files_with_errors=$BADFILES classes=$CLASSES compiler_exit=$COMPILER_EXIT"
source "$ROOT/tests/measure_result.sh"
validate_measure_result $COMPILER_EXIT $ERRORS $CLASSES ${#FILES[@]} "$LOG"
