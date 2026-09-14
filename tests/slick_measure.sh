#!/bin/zsh
# Compile slick's main sources with scala-rs and report the error count.
# Usage: tests/slick_measure.sh [extra scala-rs args...]
set -e
ROOT=${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}
source "$ROOT/tests/fixture_cache.sh"
SP=${SLICK_FIXTURE_DIR:-$(fixture_path slick)}
typeset -a FIND_RESULT
find_sorted() {
  FIND_RESULT=( "${(@0)$(find "$@" -print0 | sort -z)}" )
  FIND_RESULT=( "${(@)FIND_RESULT:#}" )
}
# --- self-restore -----------------------------------------------------------
# Everything under /tmp vanishes on reboot (it did, 2026-08-31). Rebuild the
# toolchain from the Coursier cache and re-clone slick at the pinned revision
# whenever a piece is missing, so a reboot costs one slow run instead of a
# debugging session.
SLICK_REV=$(fixture_cfg slick revision)
SCALA_VERSION=$(fixture_toolchain_version)
CCACHE=$HOME/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2
slick_expected_deps() {
  local -a jars
  local jar
  jars=(
    "$CCACHE/com/typesafe/config/1.4.9/config-1.4.9.jar"
    "$CCACHE/org/reactivestreams/reactive-streams/1.0.4/reactive-streams-1.0.4.jar"
    "$CCACHE/org/slf4j/slf4j-api/2.0.18/slf4j-api-2.0.18.jar"
    "$CCACHE/org/typelevel/cats-core_2.13/2.13.0/cats-core_2.13-2.13.0.jar"
    "$CCACHE/org/typelevel/cats-kernel_2.13/2.13.0/cats-kernel_2.13-2.13.0.jar"
    "$CCACHE/org/typelevel/cats-effect_2.13/3.7.1/cats-effect_2.13-3.7.1.jar"
    "$CCACHE/org/typelevel/cats-effect-kernel_2.13/3.7.1/cats-effect-kernel_2.13-3.7.1.jar"
    "$CCACHE/org/typelevel/cats-effect-std_2.13/3.7.1/cats-effect-std_2.13-3.7.1.jar"
    "$CCACHE/org/typelevel/cats-mtl_2.13/1.6.0/cats-mtl_2.13-1.6.0.jar"
    "$CCACHE/org/scodec/scodec-bits_2.13/1.2.4/scodec-bits_2.13-1.2.4.jar"
    "$CCACHE/co/fs2/fs2-core_2.13/3.13.0/fs2-core_2.13-3.13.0.jar"
  )
  for jar in "${jars[@]}"; do
    [[ -s $jar ]] || {
      print -u2 "fixture dependency is missing for slick: $jar"
      return 1
    }
  done
  print -r -- ${(j.:.)jars}
}
fixture_prepare_toolchain "$CCACHE"
fixture_require_checkout slick "$SP/slick"
DEPS_MARK=$SP/.fixture-deps
DEPS_EXPECTED=$(slick_expected_deps)
DEPS_CURRENT=$(cat "$SP/deps.cp" 2>/dev/null || true)
DEPS_CONTENT_DIGEST=$(fixture_dependency_digest "$SP/deps.cp" 2>/dev/null || print missing)
if ! fixture_state_valid "$DEPS_MARK" slick "$SLICK_REV" "$SLICK_REV" "$DEPS_CONTENT_DIGEST" classpath || [[ $DEPS_CURRENT != "$DEPS_EXPECTED" ]]; then
  DEPS_LOCK=$SP/.fixture-deps.lock
  fixture_lock_acquire "$DEPS_LOCK" "$SP" || exit 1
  DEPS_CURRENT=$(cat "$SP/deps.cp" 2>/dev/null || true)
  DEPS_CONTENT_DIGEST=$(fixture_dependency_digest "$SP/deps.cp" 2>/dev/null || print missing)
  if ! fixture_state_valid "$DEPS_MARK" slick "$SLICK_REV" "$SLICK_REV" "$DEPS_CONTENT_DIGEST" classpath || [[ $DEPS_CURRENT != "$DEPS_EXPECTED" ]]; then
  print -r -- "$DEPS_EXPECTED" > "$SP/deps.cp.$$"
  mv -f "$SP/deps.cp.$$" "$SP/deps.cp"
  fi
  DEPS_CURRENT=$(cat "$SP/deps.cp")
  if [[ $DEPS_CURRENT != "$DEPS_EXPECTED" ]] || ! fixture_validate_classpath "$DEPS_CURRENT"; then
    fixture_lock_release "$DEPS_LOCK" "$SP"
    print -u2 "measurement invalid: slick dependency classpath is not the pinned list"
    exit 1
  fi
  DEPS_CONTENT_DIGEST=$(fixture_dependency_digest "$SP/deps.cp")
  fixture_state_mark "$DEPS_MARK" slick "$SLICK_REV" "$SLICK_REV" "$DEPS_CONTENT_DIGEST" classpath
  fixture_lock_release "$DEPS_LOCK" "$SP"
fi
# ---------------------------------------------------------------------------
SRC=$SP/slick/slick/src/main
COMPAT=$SP/slick/slick-compat-collections/src/main/scala-2.13+
REFLECT=$(fixture_toolchain_reflect)
SCALAC=$(fixture_scalac_path)
LIB=$(fixture_toolchain_library)
# Default to *this* checkout's binary, not a fixed path: run from a git
# worktree, the hardcoded path measured the parent repo's build and an agent's
# own changes appeared to do nothing.
BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
# The release binary is not what `cargo test` builds; measuring a stale one
# silently reports the previous commit's numbers.
if [[ -z ${SCALA_RS:-} ]]; then
  mkdir -p "$FIXTURE_ROOT/logs/slick"
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>"$FIXTURE_ROOT/logs/slick/build-$$.log" \
    || { cat "$FIXTURE_ROOT/logs/slick/build-$$.log"; exit 1; }
fi
# slick keeps seven sources as FreeMarker templates that its own build
# expands. Measuring without them reports errors scalac would report too, so
# expand them here and compile them alongside.
# Every path this script writes is per-invocation: two measurements running at
# once (an agent's copy and this one) shared `generated/`, `measure-out/` and
# `measure.txt`, and reported each other's numbers.
RUN=${SLICK_RUN:-$SP/run-$$}
GEN=${SLICK_GEN:-$RUN/generated}
if [[ -n ${SLICK_GEN:-} ]]; then rm -rf -- "$GEN"; else fixture_safe_clean "$RUN" "$GEN"; fi
python3 "$ROOT/tests/expand_fm.py" "$SRC/scala" "$GEN" >/dev/null
find_sorted "$SRC/scala" -type f
FM_INPUTS=("${FIND_RESULT[@]}")
FM_DIGEST=$(fixture_source_digest "$FIXTURE_ROOT" "${FM_INPUTS[@]}")
GEN_COUNT=$(find "$GEN" -type f -name '*.scala' | wc -l | tr -d ' ')
GEN_STATE=$(fixture_cfg slick generated_state)
fixture_validate_generated slick "$GEN_COUNT" "$GEN_STATE"
GEN_CONTENT_DIGEST=$(fixture_generated_content_digest "$GEN" "$GEN")
fixture_generated_mark slick "$GEN" "$SLICK_REV" "$FM_DIGEST" "$GEN_COUNT" "$GEN_STATE" "$GEN_CONTENT_DIGEST"
find_sorted "$SRC/scala" "$SRC/scala-2" "$COMPAT" "$GEN" -name '*.scala'
FILES=("${FIND_RESULT[@]}")
fixture_validate_source_count slick ${#FILES[@]} default
OUT=${SLICK_OUT:-$RUN/out}
CP=$(cat "$SP/deps.cp"):$REFLECT
KEY_CP="$CP:$LIB"
fixture_validate_classpath "$KEY_CP"
SOURCE_DIGEST=$(fixture_source_digest "$FIXTURE_ROOT" "${FILES[@]}")
FLAGS="-Xsource:3 --scala-library $SCALA_VERSION"
FLAGS+=" args_sha256=$(fixture_argument_vector_digest "$@")"
ARTIFACT_KEY=$(fixture_artifact_key slick scala-rs "$BIN" "$SLICK_REV" "$SOURCE_DIGEST" "$FLAGS" "$KEY_CP")
ARTIFACT_DIR=$(fixture_cache_path slick scala-rs "$ARTIFACT_KEY")
ARTIFACT_HIT=0
if fixture_artifact_valid "$ARTIFACT_DIR" "$ARTIFACT_KEY" "${#FILES[@]}" slick scala-rs "$BIN" "$SLICK_REV" "$SOURCE_DIGEST" "$FLAGS" "$KEY_CP"; then
  ARTIFACT_HIT=1
  if [[ -n ${SLICK_OUT:-} ]]; then
    rm -rf -- "$OUT"; mkdir -p "$OUT"
    cp -R "$ARTIFACT_DIR/out/." "$OUT/"
  else
    OUT=$ARTIFACT_DIR/out
  fi
  print "cache: slick scala-rs artifact hit key=$ARTIFACT_KEY"
else
  if [[ -n ${SLICK_OUT:-} ]]; then rm -rf -- "$OUT"; else fixture_safe_clean "$RUN" "$OUT"; fi
  mkdir -p "$OUT"
fi
mkdir -p "$FIXTURE_ROOT/logs/slick"
LOG=${SLICK_LOG:-$FIXTURE_ROOT/logs/slick/measure-$$.txt}
# slick's build.sbt depends on scala-reflect (its macros import
# scala.reflect.macros.blackbox.Context); without the jar even real scalac
# cannot compile ShapedValue.scala, so measuring without it asks for the
# impossible. Appended here rather than in the shared deps.cp so a stale
# fixture state cannot lose it.
COMPILER_EXIT=0
if (( ARTIFACT_HIT )); then
  : > "$LOG"
else
  "$BIN" compile "${FILES[@]}" -d "$OUT" -cp "$CP" -Xsource:3 \
    --scala-library "$LIB" "$@" > "$LOG" 2>&1 || COMPILER_EXIT=$?
fi
ERRORS=$(grep -c '^error' "$LOG" || true)
CLASSES=$(find "$OUT" -name '*.class' | wc -l | tr -d ' ')
# Cascades inflate the raw count; files-with-errors is the honest progress metric.
BADFILES=$(grep -A 2 '^error' "$LOG" | grep -oE '(src/main|generated)/[^:]*' | sort -u | wc -l | tr -d ' ')
if (( COMPILER_EXIT == 0 && ERRORS == 0 && CLASSES > 0 && ARTIFACT_HIT == 0 )); then
  STAGE=$(fixture_artifact_stage slick scala-rs "$ARTIFACT_KEY")
  cp -R "$OUT" "$STAGE/out"
  fixture_artifact_publish "$STAGE" "$ARTIFACT_DIR" "$ARTIFACT_KEY" \
    "fixture=slick" "kind=scala-rs" "source_count=${#FILES[@]}" "compiler_hash=$(fixture_file_hash "$BIN")" \
    "upstream_revision=$SLICK_REV" "source_digest=$SOURCE_DIGEST" \
    "flags=$FLAGS" "classpath_digest=$(fixture_classpath_digest "$KEY_CP")" \
    "scala_toolchain=$(fixture_toolchain_digest)" \
    "jdk=$(fixture_jdk_fingerprint)" "scalac=$SCALAC" \
    "scalac_hash=$(fixture_file_hash "$SCALAC")"
fi
if [[ ${SLICK_OUT:-} == "" ]]; then
  if fixture_path_is_child "$SP" "$RUN"; then fixture_safe_clean "$SP" "$RUN"; fi
fi
echo "files=${#FILES[@]} errors=$ERRORS files_with_errors=$BADFILES classes=$CLASSES compiler_exit=$COMPILER_EXIT"
source "$ROOT/tests/measure_result.sh"
validate_measure_result $COMPILER_EXIT $ERRORS $CLASSES ${#FILES[@]} "$LOG"
