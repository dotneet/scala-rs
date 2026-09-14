#!/bin/zsh
# Compile gitbucket's main sources with scala-rs and report the error count.
# Usage: tests/gitbucket_measure.sh [extra scala-rs args...]
#
# Modelled on tests/slick_measure.sh and tests/cats_measure.sh. Same rules:
#   * pin the revision, and re-fetch the material when it is missing, so a
#     reboot that wipes /tmp costs one slow run and not a debugging session;
#   * every path this script writes is per-invocation, so two measurements
#     running at once cannot report each other's numbers;
#   * point GITBUCKET_LOG at a path of your own; the default is per-invocation.
set -e
ROOT=${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}
source "$ROOT/tests/fixture_cache.sh"
SP=${GITBUCKET_FIXTURE_DIR:-$(fixture_path gitbucket)}
typeset -a FIND_RESULT
find_sorted() {
  FIND_RESULT=( "${(@0)$(find "$@" -print0 | sort -z)}" )
  FIND_RESULT=( "${(@)FIND_RESULT:#}" )
}
# master as of 2026-09-04, "Update scala3-library to 3.9.0"; build.sbt says
# version 4.48.0, scalaVersion 2.13.18.
GITBUCKET_REV=$(fixture_cfg gitbucket revision)
SCALA_VERSION=$(fixture_toolchain_version)
CCACHE=$HOME/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2
fixture_prepare_toolchain "$CCACHE"
SCALAC=$(fixture_scalac_path)
LIB=$(fixture_toolchain_library)
REFLECT=$(fixture_toolchain_reflect)
JAVAC_BIN=$(fixture_javac_path)
SRCROOT=$SP/gitbucket
fixture_require_checkout gitbucket "$SRCROOT"
# gitbucket writes 139 of its 354 sources with the Twirl template compiler
# (sbt-twirl turns src/main/twirl/**/*.scala.html into Scala). Measuring
# without them asks for a source set real scalac never sees, so let sbt write
# them once, and take the dependency classpath from the same run rather than
# hand-listing 100 jars.
TWIRL=$SRCROOT/target/scala-2.13/twirl/main
find_sorted "$SRCROOT/src/main/twirl" -type f
TWIRL_INPUTS=("${FIND_RESULT[@]}")
TWIRL_DIGEST=$(fixture_source_digest "$FIXTURE_ROOT" "${TWIRL_INPUTS[@]}")
TWIRL_EXPECTED=$(fixture_cfg gitbucket generated_count)
TWIRL_STATE=$(fixture_cfg gitbucket generated_state)
TWIRL_CURRENT=$(find "$TWIRL" -type f -name '*.scala' 2>/dev/null | wc -l | tr -d ' ')
TWIRL_CONTENT_DIGEST=$(fixture_generated_content_digest "$TWIRL" "$TWIRL" 2>/dev/null || print missing)
DEPS_MARK=$SP/.fixture-deps
DEPS_CONTENT_DIGEST=$(fixture_dependency_digest "$SP/deps.cp" 2>/dev/null || print missing)
if ! fixture_generated_valid gitbucket "$TWIRL" "$GITBUCKET_REV" "$TWIRL_DIGEST" "$TWIRL_EXPECTED" "$TWIRL_STATE" "$TWIRL_CONTENT_DIGEST" || [[ $TWIRL_CURRENT != $TWIRL_EXPECTED || ! -s $SP/deps.cp ]] || ! fixture_state_valid "$DEPS_MARK" gitbucket "$GITBUCKET_REV" "$TWIRL_DIGEST" "$DEPS_CONTENT_DIGEST" classpath; then
  DEPS_LOCK=$SP/.fixture-deps.lock
  fixture_lock_acquire "$DEPS_LOCK" "$SP" || exit 1
  # Recheck under the lock because another first-use process may have
  # completed both Twirl and dependency setup while this process waited.
  TWIRL_CURRENT=$(find "$TWIRL" -type f -name '*.scala' 2>/dev/null | wc -l | tr -d ' ')
  TWIRL_CONTENT_DIGEST=$(fixture_generated_content_digest "$TWIRL" "$TWIRL" 2>/dev/null || print missing)
  DEPS_CONTENT_DIGEST=$(fixture_dependency_digest "$SP/deps.cp" 2>/dev/null || print missing)
  if ! fixture_generated_valid gitbucket "$TWIRL" "$GITBUCKET_REV" "$TWIRL_DIGEST" "$TWIRL_EXPECTED" "$TWIRL_STATE" "$TWIRL_CONTENT_DIGEST" || [[ $TWIRL_CURRENT != $TWIRL_EXPECTED || ! -s $SP/deps.cp ]] || ! fixture_state_valid "$DEPS_MARK" gitbucket "$GITBUCKET_REV" "$TWIRL_DIGEST" "$DEPS_CONTENT_DIGEST" classpath; then
  mkdir -p "$FIXTURE_ROOT/logs/gitbucket"
  SBT_LOG=${GITBUCKET_SBT_LOG:-$FIXTURE_ROOT/logs/gitbucket/sbt-$$.log}
  (cd "$SRCROOT" && sbt -batch -Dsbt.supershell=false \
     "Compile/managedSources" "export Compile/dependencyClasspath") > $SBT_LOG 2>&1
  # The last line of a successful `export` is the classpath. Drop
  # scala-library: it is passed separately with --scala-library, and sbt
  # resolves 2.13.18 while we link against 2.13.16.
  tail -1 "$SBT_LOG" | tr ':' '\n' | grep -v '/scala-library/' | paste -sd: - > "$SP/deps.cp.$$"
  mv -f "$SP/deps.cp.$$" "$SP/deps.cp"
  TWIRL_COUNT=$(find "$TWIRL" -type f -name '*.scala' | wc -l | tr -d ' ')
  fixture_validate_generated gitbucket "$TWIRL_COUNT" "$TWIRL_STATE"
  TWIRL_CONTENT_DIGEST=$(fixture_generated_content_digest "$TWIRL" "$TWIRL")
  fixture_generated_mark gitbucket "$TWIRL" "$GITBUCKET_REV" "$TWIRL_DIGEST" "$TWIRL_COUNT" "$TWIRL_STATE" "$TWIRL_CONTENT_DIGEST"
  DEPS_CONTENT_DIGEST=$(fixture_dependency_digest "$SP/deps.cp")
  fixture_state_mark "$DEPS_MARK" gitbucket "$GITBUCKET_REV" "$TWIRL_DIGEST" "$DEPS_CONTENT_DIGEST" classpath
  fi
  fixture_lock_release "$DEPS_LOCK" "$SP"
fi
# ---------------------------------------------------------------------------
# Default to *this* checkout's binary, not a fixed path: run from a git
# worktree, a hardcoded path measures the parent repo's build and an agent's
# own changes appear to do nothing.
BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
if [[ -z ${SCALA_RS:-} ]]; then
  mkdir -p "$FIXTURE_ROOT/logs/gitbucket"
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>"$FIXTURE_ROOT/logs/gitbucket/build-$$.log" \
    || { cat "$FIXTURE_ROOT/logs/gitbucket/build-$$.log"; exit 1; }
fi
# GITBUCKET_MODULES picks the source set: `scala+twirl` (default, the whole
# Compile configuration), `scala` (hand-written sources only), or `twirl`
# (generated templates only, which do not typecheck without the hand-written
# sources and are only useful for isolating template-specific symptoms).
MODULES=${GITBUCKET_MODULES:-scala+twirl}
DIRS=()
[[ $MODULES == *scala* ]] && DIRS+=("$SRCROOT/src/main/scala")
[[ $MODULES == *twirl* ]] && DIRS+=("$TWIRL")
# The parser now lowers value definitions followed by guards. Include the
# complete source set; an explicit exclusion remains useful for comparisons
# against historical 353-source measurements.
EXCLUDE=${GITBUCKET_EXCLUDE-}
find_sorted "${DIRS[@]}" -name '*.scala'
ALL=("${FIND_RESULT[@]}")
case $MODULES in
  scala+twirl) fixture_validate_source_count gitbucket ${#ALL[@]} default ;;
  scala) fixture_validate_source_count gitbucket ${#ALL[@]} scala ;;
  twirl) fixture_validate_source_count gitbucket ${#ALL[@]} twirl ;;
  *) print -u2 "measurement invalid: unknown GITBUCKET_MODULES=$MODULES"; exit 1 ;;
esac
GEN_COUNT=$(find "$TWIRL" -type f -name '*.scala' | wc -l | tr -d ' ')
fixture_validate_generated gitbucket "$GEN_COUNT" "$TWIRL_STATE"
TWIRL_CONTENT_DIGEST=$(fixture_generated_content_digest "$TWIRL" "$TWIRL")
fixture_generated_valid gitbucket "$TWIRL" "$GITBUCKET_REV" "$TWIRL_DIGEST" "$GEN_COUNT" "$TWIRL_STATE" "$TWIRL_CONTENT_DIGEST"
DEPS_CONTENT_DIGEST=$(fixture_dependency_digest "$SP/deps.cp")
fixture_state_valid "$DEPS_MARK" gitbucket "$GITBUCKET_REV" "$TWIRL_DIGEST" "$DEPS_CONTENT_DIGEST" classpath
if [[ -n $EXCLUDE ]]; then
  FILES=("${(@)ALL:#*$EXCLUDE}")
else
  FILES=("${ALL[@]}")
fi
SKIPPED=$(( ${#ALL[@]} - ${#FILES[@]} ))
RUN=${GITBUCKET_RUN:-$SP/run-$$}
OUT=${GITBUCKET_OUT:-$RUN/out}
if [[ -n ${GITBUCKET_OUT:-} ]]; then rm -rf -- "$OUT"; else fixture_safe_clean "$RUN" "$OUT"; fi
mkdir -p "$OUT"
mkdir -p "$FIXTURE_ROOT/logs/gitbucket"
LOG=${GITBUCKET_LOG:-$FIXTURE_ROOT/logs/gitbucket/measure-$$.txt}
# gitbucket's own scalacOptions, minus the warning settings that only affect
# reporting (-deprecation -feature -Werror -Wunused:imports -Wconf) and the
# optimiser (-opt:l:method). `-Xsource:3-cross` is load-bearing: gitbucket
# cross-builds for Scala 3 and the source relies on 3's rules.
# gitbucket calls slick's `TableQuery` / `mapTo` macros, and running a macro
# implementation needs `scala.reflect.runtime.universe`. Real scalac has it
# because scala-reflect.jar is part of the compiler's own classpath, not the
# project's -- sbt never puts it on gitbucket's. scala-rs has no such classpath
# of its own, so the jar is appended here for the same reason
# `tests/slick_measure.sh` appends it: measuring without it asks for a macro
# expansion nobody could perform.
# Compile the project's actual Java helpers before Scala, as the mixed sbt
# build does. Per-invocation output prevents stale classes from masking errors.
# GITBUCKET_JAVA=0 exists only for comparable historical-input measurements.
DEPS=$(cat "$SP/deps.cp")
JAVA_FILES=("$SRCROOT"/src/main/java/**/*.java(N))
JAVA_COUNT=0
if [[ ${GITBUCKET_JAVA:-1} == 1 ]]; then
  JAVA_OUT=$RUN/java
  mkdir -p "$JAVA_OUT"
  JAVA_EXPECTED=$(fixture_cfg gitbucket java_source_count)
  if (( ${#JAVA_FILES[@]} != JAVA_EXPECTED )); then
    print "measurement invalid: expected $JAVA_EXPECTED gitbucket Java sources, found ${#JAVA_FILES[@]}"
    exit 1
  fi
  "$JAVAC_BIN" -cp "$DEPS:$LIB" -d "$JAVA_OUT" "${JAVA_FILES[@]}" > "$RUN/javac.log" 2>&1 \
    || { cat "$RUN/javac.log"; print "measurement invalid: gitbucket Java compilation failed"; exit 1; }
  DEPS="$JAVA_OUT:$DEPS"
  JAVA_COUNT=${#JAVA_FILES[@]}
fi
KEY_CP="$DEPS:$REFLECT:$LIB"
fixture_validate_classpath "$KEY_CP"
SOURCE_DIGEST=$(fixture_source_digest "$FIXTURE_ROOT" "${FILES[@]}" "${JAVA_FILES[@]}")
FLAGS="-Xsource:3-cross -language:postfixOps --scala-library $SCALA_VERSION"
FLAGS+=" args_sha256=$(fixture_argument_vector_digest "$@")"
ARTIFACT_KEY=$(fixture_artifact_key gitbucket scala-rs "$BIN" "$GITBUCKET_REV" "$SOURCE_DIGEST" "$FLAGS" "$KEY_CP")
ARTIFACT_DIR=$(fixture_cache_path gitbucket scala-rs "$ARTIFACT_KEY")
ARTIFACT_HIT=0
if fixture_artifact_valid "$ARTIFACT_DIR" "$ARTIFACT_KEY" "${#FILES[@]}" gitbucket scala-rs "$BIN" "$GITBUCKET_REV" "$SOURCE_DIGEST" "$FLAGS" "$KEY_CP"; then
  ARTIFACT_HIT=1
  if [[ -n ${GITBUCKET_OUT:-} ]]; then
    rm -rf -- "$OUT"; mkdir -p "$OUT"
    cp -R "$ARTIFACT_DIR/out/." "$OUT/"
  else
    OUT=$ARTIFACT_DIR/out
  fi
  print "cache: gitbucket scala-rs artifact hit key=$ARTIFACT_KEY"
else
  if [[ -n ${GITBUCKET_OUT:-} ]]; then rm -rf -- "$OUT"; else fixture_safe_clean "$RUN" "$OUT"; fi
  mkdir -p "$OUT"
fi
COMPILER_EXIT=0
if (( ARTIFACT_HIT )); then
  : > "$LOG"
else
  "$BIN" compile "${FILES[@]}" -d "$OUT" -cp "$DEPS:$REFLECT" -Xsource:3-cross \
    -language:postfixOps \
    --scala-library "$LIB" "$@" > "$LOG" 2>&1 || COMPILER_EXIT=$?
fi
ERRORS=$(grep -c '^error' "$LOG" || true)
CLASSES=$(find "$OUT" -name '*.class' | wc -l | tr -d ' ')
# Cascades inflate the raw count; files-with-errors is the honest metric.
BADFILES=$(grep -A 2 '^error' "$LOG" | grep -oE '(src/main|twirl/main)/[^:]*' | sort -u | wc -l | tr -d ' ')
if (( COMPILER_EXIT == 0 && ERRORS == 0 && CLASSES > 0 && ARTIFACT_HIT == 0 )); then
  STAGE=$(fixture_artifact_stage gitbucket scala-rs "$ARTIFACT_KEY")
  cp -R "$OUT" "$STAGE/out"
  fixture_artifact_publish "$STAGE" "$ARTIFACT_DIR" "$ARTIFACT_KEY" \
    "fixture=gitbucket" "kind=scala-rs" "source_count=${#FILES[@]}" "compiler_hash=$(fixture_file_hash "$BIN")" \
    "upstream_revision=$GITBUCKET_REV" "source_digest=$SOURCE_DIGEST" \
    "flags=$FLAGS" "classpath_digest=$(fixture_classpath_digest "$KEY_CP")" \
    "scala_toolchain=$(fixture_toolchain_digest)" \
    "jdk=$(fixture_jdk_fingerprint)" "scalac=$SCALAC" \
    "scalac_hash=$(fixture_file_hash "$SCALAC")"
fi
if [[ ${GITBUCKET_OUT:-} == "" ]]; then
  if fixture_path_is_child "$SP" "$RUN"; then fixture_safe_clean "$SP" "$RUN"; fi
fi
echo "files=${#FILES[@]} skipped=$SKIPPED errors=$ERRORS files_with_errors=$BADFILES classes=$CLASSES compiler_exit=$COMPILER_EXIT java_sources=$JAVA_COUNT"
source "$ROOT/tests/measure_result.sh"
validate_measure_result $COMPILER_EXIT $ERRORS $CLASSES ${#FILES[@]} "$LOG"
