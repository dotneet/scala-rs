#!/bin/zsh
# Compile typelevel/cats' kernel + core main sources with scala-rs and report
# the error count. Usage: tests/cats_measure.sh [extra scala-rs args...]
#
# Modelled on tests/slick_measure.sh. Same rules apply:
#   * pin the revision, and re-fetch the material when it is missing, so a
#     reboot that wipes /tmp costs one slow run and not a debugging session;
#   * every path this script writes is per-invocation, so two measurements
#     running at once cannot report each other's numbers;
#   * point CATS_LOG at a path of your own; the default is per-invocation.
set -e
ROOT=${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}
source "$ROOT/tests/fixture_cache.sh"
SP=${CATS_FIXTURE_DIR:-$(fixture_path cats)}
typeset -a FIND_RESULT
find_sorted() {
  FIND_RESULT=( "${(@0)$(find "$@" -print0 | sort -z)}" )
  FIND_RESULT=( "${(@)FIND_RESULT:#}" )
}
# v2.13.0, the release whose published jars are in the Coursier cache.  The
# revision and source expectations are centralized in fixture_manifest.toml.
CATS_REV=$(fixture_cfg cats revision)
SCALA_VERSION=$(fixture_toolchain_version)
CCACHE=$HOME/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2
cats_expected_deps() {
  local -a jars
  local jar
  jars=(
    "$CCACHE/org/typelevel/scalac-compat-annotation_2.13/0.1.4/scalac-compat-annotation_2.13-0.1.4.jar"
    "$CCACHE/org/scala-lang/scala-reflect/$SCALA_VERSION/scala-reflect-$SCALA_VERSION.jar"
  )
  for jar in "${jars[@]}"; do
    [[ -s $jar ]] || {
      print -u2 "fixture dependency is missing for cats: $jar"
      return 1
    }
  done
  print -r -- ${(j.:.)jars}
}
fixture_prepare_toolchain "$CCACHE"
fixture_require_checkout cats "$SP/cats"
# cats generates part of kernel and core with sbt source generators
# (project/KernelBoiler.scala, project/Boilerplate.scala): 1 file for kernel,
# 15 for core. Measuring without them asks for a source set real scalac never
# sees, so let sbt write them once and compile them alongside.
KGEN=$SP/cats/kernel/.jvm/target/scala-2.13/src_managed/main
CGEN=$SP/cats/core/.jvm/target/scala-2.13/src_managed/main
find_sorted "$SP/cats/kernel/src/main/scala" "$SP/cats/kernel/src/main/scala-2.13+" \
  "$SP/cats/core/src/main/scala" "$SP/cats/core/src/main/scala-2" \
  "$SP/cats/core/src/main/scala-2.13+" -name '*.scala'
GEN_INPUTS=("${FIND_RESULT[@]}")
GEN_INPUT_DIGEST=$(fixture_source_digest "$FIXTURE_ROOT" "${GEN_INPUTS[@]}")
GEN_EXPECTED=$(fixture_cfg cats generated_count)
GEN_STATE=$(fixture_cfg cats generated_state)
GEN_CURRENT=$(find "$KGEN" "$CGEN" -type f -name '*.scala' 2>/dev/null | wc -l | tr -d ' ')
GEN_CONTENT_DIGEST=$(fixture_generated_content_digest "$SP/cats" "$KGEN" "$CGEN" 2>/dev/null || print missing)
if [[ ! -d $KGEN || ! -d $CGEN || $GEN_CURRENT != $GEN_EXPECTED ]] || ! fixture_generated_valid cats "$SP/cats" "$CATS_REV" "$GEN_INPUT_DIGEST" "$GEN_EXPECTED" "$GEN_STATE" "$GEN_CONTENT_DIGEST"; then
  GEN_LOCK=$SP/cats/.fixture-generated.lock
  fixture_lock_acquire "$GEN_LOCK" "$SP/cats" || exit 1
  GEN_CURRENT=$(find "$KGEN" "$CGEN" -type f -name '*.scala' 2>/dev/null | wc -l | tr -d ' ')
  GEN_CONTENT_DIGEST=$(fixture_generated_content_digest "$SP/cats" "$KGEN" "$CGEN" 2>/dev/null || print missing)
  if [[ ! -d $KGEN || ! -d $CGEN || $GEN_CURRENT != $GEN_EXPECTED ]] || ! fixture_generated_valid cats "$SP/cats" "$CATS_REV" "$GEN_INPUT_DIGEST" "$GEN_EXPECTED" "$GEN_STATE" "$GEN_CONTENT_DIGEST"; then
  (cd "$SP/cats" && sbt -batch -Dsbt.supershell=false \
     "kernelJVM/Compile/managedSources" "coreJVM/Compile/managedSources") >/dev/null 2>&1
  GEN_COUNT=$(find "$KGEN" "$CGEN" -type f -name '*.scala' | wc -l | tr -d ' ')
  fixture_validate_generated cats "$GEN_COUNT" "$GEN_STATE"
  GEN_CONTENT_DIGEST=$(fixture_generated_content_digest "$SP/cats" "$KGEN" "$CGEN")
  fixture_generated_mark cats "$SP/cats" "$CATS_REV" "$GEN_INPUT_DIGEST" "$GEN_COUNT" "$GEN_STATE" "$GEN_CONTENT_DIGEST"
  fi
  fixture_lock_release "$GEN_LOCK" "$SP/cats"
fi
DEPS_MARK=$SP/.fixture-deps
DEPS_EXPECTED=$(cats_expected_deps)
DEPS_CURRENT=$(cat "$SP/deps.cp" 2>/dev/null || true)
DEPS_INPUT_DIGEST=$CATS_REV
DEPS_CONTENT_DIGEST=$(fixture_dependency_digest "$SP/deps.cp" 2>/dev/null || print missing)
if ! fixture_state_valid "$DEPS_MARK" cats "$CATS_REV" "$DEPS_INPUT_DIGEST" "$DEPS_CONTENT_DIGEST" classpath || [[ $DEPS_CURRENT != "$DEPS_EXPECTED" ]]; then
  DEPS_LOCK=$SP/.fixture-deps.lock
  fixture_lock_acquire "$DEPS_LOCK" "$SP" || exit 1
  DEPS_CURRENT=$(cat "$SP/deps.cp" 2>/dev/null || true)
  DEPS_CONTENT_DIGEST=$(fixture_dependency_digest "$SP/deps.cp" 2>/dev/null || print missing)
  if ! fixture_state_valid "$DEPS_MARK" cats "$CATS_REV" "$DEPS_INPUT_DIGEST" "$DEPS_CONTENT_DIGEST" classpath || [[ $DEPS_CURRENT != "$DEPS_EXPECTED" ]]; then
  # What sbt reports for coreJVM/Compile/dependencyClasspath, minus
  # scala-library (passed separately with --scala-library).
  print -r -- "$DEPS_EXPECTED" > "$SP/deps.cp.$$"
  mv -f "$SP/deps.cp.$$" "$SP/deps.cp"
  fi
  DEPS_CURRENT=$(cat "$SP/deps.cp")
  if [[ $DEPS_CURRENT != "$DEPS_EXPECTED" ]] || ! fixture_validate_classpath "$DEPS_CURRENT"; then
    fixture_lock_release "$DEPS_LOCK" "$SP"
    print -u2 "measurement invalid: cats dependency classpath is not the pinned list"
    exit 1
  fi
  DEPS_CONTENT_DIGEST=$(fixture_dependency_digest "$SP/deps.cp")
  fixture_state_mark "$DEPS_MARK" cats "$CATS_REV" "$DEPS_INPUT_DIGEST" "$DEPS_CONTENT_DIGEST" classpath
  fixture_lock_release "$DEPS_LOCK" "$SP"
fi
# ---------------------------------------------------------------------------
# The cross-version source directories sbt selects for 2.13 (`scala`,
# `scala-2`, `scala-2.13+`); `scala-2.12` and `scala-3` are not compiled.
BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
SCALAC=$(fixture_scalac_path)
LIB=$(fixture_toolchain_library)
if [[ -z ${SCALA_RS:-} ]]; then
  mkdir -p "$FIXTURE_ROOT/logs/cats"
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>"$FIXTURE_ROOT/logs/cats/build-$$.log" \
    || { cat "$FIXTURE_ROOT/logs/cats/build-$$.log"; exit 1; }
fi
# CATS_MODULES picks the source set: `kernel+core` (default, both from source),
# `kernel`, or `core` (core from source against the published cats-kernel jar,
# which is how sbt builds it and the only way to see core's own numbers without
# kernel's in them).
MODULES=${CATS_MODULES:-kernel+core}
DIRS=()
EXTRA_CP=
if [[ $MODULES == *kernel* ]]; then
  DIRS+=("$SP/cats/kernel/src/main/scala" "$SP/cats/kernel/src/main/scala-2.13+" "$KGEN")
else
  EXTRA_CP=:$CCACHE/org/typelevel/cats-kernel_2.13/2.13.0/cats-kernel_2.13-2.13.0.jar
fi
if [[ $MODULES == *core* ]]; then
  DIRS+=("$SP/cats/core/src/main/scala" "$SP/cats/core/src/main/scala-2" \
         "$SP/cats/core/src/main/scala-2.13+" "$CGEN")
fi
# CATS_EXCLUDE holds files out of the measurement; nothing is held out now.
#
# It used to default to `FunctionKMacros.scala`, cats' one macro
# implementation: it matches trees with quasiquote *patterns*
# (`case q"($param) => $trans[..$typeArgs]($arg)"`), interpolated-string
# patterns in that position were not implemented at all, and a parse error
# stops the run before typing -- so that one file hid the diagnostics of the
# other 339. Quasiquote patterns are implemented
# (`crates/typer/src/quasi_pattern.rs`), the file compiles, and all 340 are
# measured. Keeping the holdout would now *create* an error, because
# `FunctionK.scala` extends the `FunctionKMacroMethods` that file declares.
EXCLUDE=${CATS_EXCLUDE-}
find_sorted "${DIRS[@]}" -name '*.scala'
ALL=("${FIND_RESULT[@]}")
case $MODULES in
  kernel+core) fixture_validate_source_count cats ${#ALL[@]} default ;;
  kernel) fixture_validate_source_count cats ${#ALL[@]} kernel ;;
  core) fixture_validate_source_count cats ${#ALL[@]} core ;;
  *) print -u2 "measurement invalid: unknown CATS_MODULES=$MODULES"; exit 1 ;;
esac
GEN_COUNT=$(find "$KGEN" "$CGEN" -type f -name '*.scala' | wc -l | tr -d ' ')
fixture_validate_generated cats "$GEN_COUNT" "$GEN_STATE"
GEN_CONTENT_DIGEST=$(fixture_generated_content_digest "$SP/cats" "$KGEN" "$CGEN")
fixture_generated_valid cats "$SP/cats" "$CATS_REV" "$GEN_INPUT_DIGEST" "$GEN_COUNT" "$GEN_STATE" "$GEN_CONTENT_DIGEST"
if [[ -n $EXCLUDE ]]; then
  FILES=("${(@)ALL:#*$EXCLUDE}")
else
  FILES=("${ALL[@]}")
fi
SKIPPED=$(( ${#ALL[@]} - ${#FILES[@]} ))
CP=$(cat "$SP/deps.cp")$EXTRA_CP
KEY_CP="$CP:$LIB"
fixture_validate_classpath "$KEY_CP"
SOURCE_DIGEST=$(fixture_source_digest "$SP/cats" "${FILES[@]}")
FLAGS="-Xsource:3 -no-specialization -Ykind-projector --scala-library $SCALA_VERSION"
FLAGS+=" args_sha256=$(fixture_argument_vector_digest "$@")"
ARTIFACT_KEY=$(fixture_artifact_key cats scala-rs "$BIN" "$CATS_REV" "$SOURCE_DIGEST" "$FLAGS" "$KEY_CP")
ARTIFACT_DIR=$(fixture_cache_path cats scala-rs "$ARTIFACT_KEY")
RUN=${CATS_RUN:-$SP/run-$$}
OUT=${CATS_OUT:-$RUN/out}
ARTIFACT_HIT=0
if fixture_artifact_valid "$ARTIFACT_DIR" "$ARTIFACT_KEY" "${#FILES[@]}" cats scala-rs "$BIN" "$CATS_REV" "$SOURCE_DIGEST" "$FLAGS" "$KEY_CP"; then
  ARTIFACT_HIT=1
  if [[ -n ${CATS_OUT:-} ]]; then
    rm -rf -- "$OUT"; mkdir -p "$OUT"
    cp -R "$ARTIFACT_DIR/out/." "$OUT/"
  else
    OUT=$ARTIFACT_DIR/out
  fi
  print "cache: cats scala-rs artifact hit key=$ARTIFACT_KEY"
else
  if [[ -n ${CATS_OUT:-} ]]; then rm -rf -- "$OUT"; else fixture_safe_clean "$RUN" "$OUT"; fi
  mkdir -p "$OUT"
fi
mkdir -p "$FIXTURE_ROOT/logs/cats"
LOG=${CATS_LOG:-$FIXTURE_ROOT/logs/cats/measure-$$.txt}
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
# `-Ykind-projector` is on by default here because cats *cannot be built without
# the plugin*: `λ[α => …]` and `F[A, *]` are kind-projector syntax, and real
# scalac rejects them too when the plugin is absent. Measuring without it
# measures a configuration nobody ships. It is also 80x faster (4 s vs 334 s on
# the same 339 files) -- the error-recovery path for the unresolved `λ`/`*`
# names is pathologically slow, which is its own bug (see docs/cats.md).
# Pass `-Yno-kind-projector-default` ... there is no such flag; to measure
# without it, edit this line.
COMPILER_EXIT=0
if (( ARTIFACT_HIT )); then
  : > "$LOG"
else
  "$BIN" compile "${FILES[@]}" -d "$OUT" -cp "$CP" -Xsource:3 \
    -no-specialization -Ykind-projector \
    --scala-library "$LIB" "$@" > "$LOG" 2>&1 || COMPILER_EXIT=$?
fi
ERRORS=$(grep -c '^error' "$LOG" || true)
CLASSES=$(find "$OUT" -name '*.class' | wc -l | tr -d ' ')
# Cascades inflate the raw count; files-with-errors is the honest metric.
BADFILES=$(grep -A 2 '^error' "$LOG" | grep -oE '(src/main|src_managed/main)/[^:]*' | sort -u | wc -l | tr -d ' ')
if (( COMPILER_EXIT == 0 && ERRORS == 0 && CLASSES > 0 && ARTIFACT_HIT == 0 )); then
  STAGE=$(fixture_artifact_stage cats scala-rs "$ARTIFACT_KEY")
  cp -R "$OUT" "$STAGE/out"
  fixture_artifact_publish "$STAGE" "$ARTIFACT_DIR" "$ARTIFACT_KEY" \
    "fixture=cats" "kind=scala-rs" "source_count=${#FILES[@]}" "compiler_hash=$(fixture_file_hash "$BIN")" \
    "upstream_revision=$CATS_REV" "source_digest=$SOURCE_DIGEST" \
    "flags=$FLAGS" "classpath_digest=$(fixture_classpath_digest "$KEY_CP")" \
    "scala_toolchain=$(fixture_toolchain_digest)" \
    "jdk=$(fixture_jdk_fingerprint)" "scalac=$SCALAC" \
    "scalac_hash=$(fixture_file_hash "$SCALAC")"
fi
if [[ ${CATS_OUT:-} == "" ]]; then
  if fixture_path_is_child "$SP" "$RUN"; then fixture_safe_clean "$SP" "$RUN"; fi
fi
echo "files=${#FILES[@]} skipped=$SKIPPED errors=$ERRORS files_with_errors=$BADFILES classes=$CLASSES compiler_exit=$COMPILER_EXIT"
source "$ROOT/tests/measure_result.sh"
validate_measure_result $COMPILER_EXIT $ERRORS $CLASSES ${#FILES[@]} "$LOG"
