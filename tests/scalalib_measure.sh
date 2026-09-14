#!/bin/zsh
# Compile scala/scala's own standard library (`src/library`) with scala-rs and
# report the error count. Usage: tests/scalalib_measure.sh [extra scala-rs args...]
#
# Modelled on tests/slick_measure.sh and tests/cats_measure.sh. Same rules:
#   * pin the revision and re-fetch when the material is missing, so a reboot
#     that wipes the fixture root costs one slow run, not a debugging session;
#   * every path this script writes is per-invocation, so two measurements
#     running at once cannot report each other's numbers;
#   * point SCALALIB_LOG at a path of your own; the default is per-invocation.
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
ROOT=${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}
source "$ROOT/tests/fixture_cache.sh"
SP=${SCALALIB_FIXTURE_DIR:-$(fixture_path scalalib)}
typeset -a FIND_RESULT
find_sorted() {
  FIND_RESULT=( "${(@0)$(find "$@" -print0 | sort -z)}" )
  FIND_RESULT=( "${(@)FIND_RESULT:#}" )
}
# v2.13.16 -- the same release as the jar the rest of the test suite links
# against, so the sources and the Java classfiles below are from one tree.
SCALA_REV=$(fixture_cfg scalalib revision)
SCALA_VERSION=$(fixture_toolchain_version)
CCACHE=$HOME/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2
SCALA_LIBRARY_JAR=$(fixture_toolchain_library)
fixture_materialize_file \
  "$CCACHE/org/scala-lang/scala-library/$SCALA_VERSION/scala-library-$SCALA_VERSION.jar" \
  "$SCALA_LIBRARY_JAR"
fixture_require_checkout scalalib "$SP/scala"
# The Java half of the library, taken from the released jar. Rebuilt whenever
# it is missing; the list is exactly `find src/library -name '*.java'`.
JAVACP=$SP/javacp/keep
JAVACP_MARK=$SP/javacp/.fixture-javacp
JAVACP_LOCK=$SP/.fixture-javacp.lock
JAVACP_INPUT_DIGEST=$(fixture_file_hash "$SCALA_LIBRARY_JAR" 2>/dev/null || print missing)
JAVACP_CONTENT_DIGEST=$(fixture_classpath_digest "$JAVACP" 2>/dev/null || print missing)
if ! fixture_state_valid "$JAVACP_MARK" scalalib "$SCALA_REV" "$JAVACP_INPUT_DIGEST" "$JAVACP_CONTENT_DIGEST" javacp; then
  fixture_lock_acquire "$JAVACP_LOCK" "$SP" || exit 1
  JAVACP_CONTENT_DIGEST=$(fixture_classpath_digest "$JAVACP" 2>/dev/null || print missing)
  if ! fixture_state_valid "$JAVACP_MARK" scalalib "$SCALA_REV" "$JAVACP_INPUT_DIGEST" "$JAVACP_CONTENT_DIGEST" javacp; then
    fixture_safe_clean "$SP" "$SP/javacp" 2>/dev/null || exit 1
    mkdir -p "$SP/javacp/all" "$JAVACP"
    unzip -q "$SCALA_LIBRARY_JAR" -d "$SP/javacp/all"
    find_sorted "$SP/scala/src/library" -type f -name '*.java'
    for source in "${FIND_RESULT[@]}"; do
      c=${source#"$SP/scala/src/library/"}
      c=${c%.java}
      mkdir -p "$JAVACP/$(dirname "$c")"
      cp "$SP/javacp/all/$c"*.class "$JAVACP/$(dirname "$c")/" 2>/dev/null || true
    done
    fixture_safe_clean "$SP" "$SP/javacp/all"
    JAVACP_CONTENT_DIGEST=$(fixture_classpath_digest "$JAVACP")
    fixture_state_mark "$JAVACP_MARK" scalalib "$SCALA_REV" "$JAVACP_INPUT_DIGEST" "$JAVACP_CONTENT_DIGEST" javacp
  fi
  fixture_lock_release "$JAVACP_LOCK" "$SP"
fi
JAVACP_CONTENT_DIGEST=$(fixture_classpath_digest "$JAVACP")
fixture_state_valid "$JAVACP_MARK" scalalib "$SCALA_REV" "$JAVACP_INPUT_DIGEST" "$JAVACP_CONTENT_DIGEST" javacp || {
  echo "scala-library Java classpath is stale or unmarked" >&2; exit 1
}
# ---------------------------------------------------------------------------
BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
# The release binary is not what `cargo test` builds; measuring a stale one
# silently reports the previous commit's numbers.
if [[ -z ${SCALA_RS:-} ]]; then
  mkdir -p "$FIXTURE_ROOT/logs/scalalib"
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>"$FIXTURE_ROOT/logs/scalalib/build-$$.log" \
    || { cat "$FIXTURE_ROOT/logs/scalalib/build-$$.log"; exit 1; }
fi
# SCALALIB_DIRS picks the source set; `src/library` is the whole point, and the
# other two (`src/reflect`, `src/compiler`) are there for when it stops being
# the bottleneck. `src/library-aux` is never compiled -- Any/AnyRef/Nothing/
# Null/Singleton are scaladoc stubs for symbols the compiler defines itself
# (build.sbt passes them as `-doc-no-compile`).
if [[ -n ${SCALALIB_DIRS:-} ]]; then
  DIRS=("${(@s: :)SCALALIB_DIRS}")
else
  DIRS=("$SP/scala/src/library")
fi
find_sorted "${DIRS[@]}" -name '*.scala'
FILES=("${FIND_RESULT[@]}")
[[ -n ${SCALALIB_DIRS:-} ]] || fixture_validate_source_count scalalib ${#FILES[@]} default
GEN_STATE=$(fixture_cfg scalalib generated_state)
fixture_validate_generated scalalib 0 "$GEN_STATE"
fixture_validate_classpath "$JAVACP"
JAVA_CLASS_EXPECTED=$(fixture_cfg scalalib java_class_count)
JAVA_CLASS_COUNT=$(find "$JAVACP" -type f -name '*.class' | wc -l | tr -d ' ')
[[ $JAVA_CLASS_COUNT -ge $JAVA_CLASS_EXPECTED ]] || {
  echo "measurement invalid: expected at least $JAVA_CLASS_EXPECTED Java library classfiles, found $JAVA_CLASS_COUNT" >&2
  exit 1
}
RUN=${SCALALIB_RUN:-$SP/run-$$}
OUT=${SCALALIB_OUT:-$RUN/out}
if [[ -n ${SCALALIB_OUT:-} ]]; then rm -rf -- "$OUT"; else fixture_safe_clean "$RUN" "$OUT"; fi
mkdir -p "$OUT"
mkdir -p "$FIXTURE_ROOT/logs/scalalib"
LOG=${SCALALIB_LOG:-$FIXTURE_ROOT/logs/scalalib/measure-$$.txt}
# Flags: build.sbt gives the library `-feature -Xlint -Wconf:... -sourcepath
# <scalaSource>` and, in CI only, `-Werror`. None of them changes what is
# accepted, so there is nothing to pass on: no -Xsource:3, no -Yrecursion, no
# -opt (the optimiser is only turned on for the bootstrap and the benchmarks).
COMPILER_EXIT=0
if [[ ${SCALALIB_MODE:-nolib} == jar ]]; then
  "$BIN" compile "${FILES[@]}" -d "$OUT" -no-specialization \
    --scala-library "$SCALA_LIBRARY_JAR" "$@" > "$LOG" 2>&1 || COMPILER_EXIT=$?
else
  "$BIN" compile "${FILES[@]}" -d "$OUT" -cp "$JAVACP" -no-specialization --no-scala-library "$@" > "$LOG" 2>&1 || COMPILER_EXIT=$?
fi
# `-no-specialization` is nsc's own flag. The library annotates with
# `@specialized` everywhere, we reject that annotation without the flag, and a
# single parse error aborts the run before any file is typechecked -- so the
# count collapses to the parse errors alone (84) and says nothing about type
# checking. Same trap as tests/cats_measure.sh; see docs/scala-library.md.
ERRORS=$(grep -c '^error' "$LOG" || true)
CLASSES=$(find "$OUT" -name '*.class' | wc -l | tr -d ' ')
# Cascades inflate the raw count; files-with-errors is the honest metric.
BADFILES=$(grep -A 2 '^error' "$LOG" | grep -oE 'src/(library|reflect|compiler)/[^:]*' | sort -u | wc -l | tr -d ' ')
if [[ -z ${SCALALIB_OUT:-} ]] && fixture_path_is_child "$SP" "$RUN"; then fixture_safe_clean "$SP" "$RUN"; fi
echo "files=${#FILES[@]} errors=$ERRORS files_with_errors=$BADFILES classes=$CLASSES compiler_exit=$COMPILER_EXIT"
source "$ROOT/tests/measure_result.sh"
validate_measure_result $COMPILER_EXIT $ERRORS $CLASSES ${#FILES[@]} "$LOG"
