#!/bin/zsh
# Differential *execution* test for gitbucket.
#
# `gitbucket_measure.sh` answers "does scala-rs accept gitbucket?" and stops at
# `errors=0 classes=1317`. Nothing in that number says gitbucket *works*: the
# macro-heavy half of the project -- slick's `TableQuery` and `mapTo`, which
# expand into the code that reads and writes every row -- typechecks, emits,
# loads and verifies whether or not what it expanded to is right. So, like
# `tests/slick_run.sh` and `tests/cats_run.sh`: compile gitbucket twice, once
# with scala-rs and once with real scalac, then compile a set of ordinary client
# programs with *real scalac* and run them against both builds, comparing stdout
# byte for byte. The clients drive gitbucket's own `Account` / `Repository` /
# `Issue` / `Label` / `IssueComment` tables against an in-memory H2 database.
#
# Each program is compiled TWICE, which separates the two ways we can be wrong:
#
#   * against scalac's gitbucket ("side B"): the client binary is beyond
#     suspicion, so a difference when it runs against our classes is a
#     **codegen** defect;
#   * against our gitbucket ("side A"): real scalac reads *our* `ScalaSignature`
#     pickles and runs slick's macros over *our* symbols, so a failure here is a
#     **pickle** defect -- the classes may run perfectly and still be unusable
#     by any downstream compilation, plugins included.
#
# A program counts as `ok` only when both compiles succeed, both runs exit 0, and
# all three stdouts are identical. The per-program line says which half failed.
#
# Usage: tests/gitbucket_run.sh [prog-name ...]
#   with no arguments every program in tests/gbrun/ is run.
#   tests/gbrun/support/*.scala is compiled with every client, not as one.
# Env:
#   GB_RUN_DIR      work dir (default: a sibling of the gitbucket checkout)
#   GB_RUN_ID       name of the private area inside it (default: a hash of
#                   $ROOT, so every worktree gets its own)
#   REUSE_RS=1      reuse the manifest-validated scala-rs artifact when present
#   REUSE_SCALAC=0  force recompiling gitbucket with real scalac (~15 s)
#   PICKLE=0        skip the side-A compiles (codegen axis only)
#   SCALA_RS=<path> use this binary instead of building one
#
# On concurrency: the reference build depends only on the gitbucket checkout and
# on real scalac, so it is shared between worktrees and published with an atomic
# rename. Everything that depends on *your* compiler lives under $WORK, which is
# private to your worktree and held under a lock for the duration. See
# `tests/slick_run.sh`'s header for what a shared work directory did the one time
# this was not the case.
set -eo pipefail
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
source "$ROOT/tests/fixture_cache.sh"
SP=${GITBUCKET_FIXTURE_DIR:-$(fixture_path gitbucket)}
typeset -a FIND_RESULT
find_sorted() {
  FIND_RESULT=( "${(@0)$(find "$@" -print0 | sort -z)}" )
  FIND_RESULT=( "${(@)FIND_RESULT:#}" )
}
DIR=${GB_RUN_DIR:-$FIXTURE_ROOT/gbrun}
SCALAC=$(fixture_scalac_path)
REFLECT=$(fixture_toolchain_reflect)
LIB=$(fixture_toolchain_library)
JAVA_BIN=$(fixture_java_path)
JAVAC_BIN=$(fixture_javac_path)
SRCROOT=$SP/gitbucket
TWIRL=$SRCROOT/target/scala-2.13/twirl/main
RES=$SRCROOT/src/main/resources
GITBUCKET_REV=$(fixture_cfg gitbucket revision)
SCALA_VERSION=$(fixture_toolchain_version)

if [[ ! -x $SCALAC || ! -s $LIB || ! -d $SRCROOT/.git || ! -s $SP/deps.cp || ! -d $TWIRL ]]; then
  echo "toolchain or gitbucket checkout missing; run tests/gitbucket_measure.sh once first (it self-restores)" >&2
  exit 1
fi
fixture_require_checkout gitbucket "$SRCROOT"

find_sorted "$SRCROOT/src/main/scala" "$TWIRL" -name '*.scala'
FILES=("${FIND_RESULT[@]}")
JAVA_FILES=("$SRCROOT"/src/main/java/**/*.java(N))
fixture_validate_source_count gitbucket ${#FILES[@]} default
GEN_COUNT=$(find "$TWIRL" -type f -name '*.scala' | wc -l | tr -d ' ')
TWIRL_STATE=$(fixture_cfg gitbucket generated_state)
fixture_validate_generated gitbucket "$GEN_COUNT" "$TWIRL_STATE"
find_sorted "$SRCROOT/src/main/twirl" -type f
TWIRL_INPUTS=("${FIND_RESULT[@]}")
TWIRL_DIGEST=$(fixture_source_digest "$FIXTURE_ROOT" "${TWIRL_INPUTS[@]}")
TWIRL_CONTENT_DIGEST=$(fixture_generated_content_digest "$TWIRL" "$TWIRL")
fixture_generated_valid gitbucket "$TWIRL" "$GITBUCKET_REV" "$TWIRL_DIGEST" "$GEN_COUNT" "$TWIRL_STATE" "$TWIRL_CONTENT_DIGEST" || {
  echo "gitbucket generated sources are stale or unmarked; run tests/gitbucket_measure.sh once first" >&2
  exit 1
}
DEPS_MARK=$SP/.fixture-deps
DEPS_CONTENT_DIGEST=$(fixture_dependency_digest "$SP/deps.cp")
fixture_state_valid "$DEPS_MARK" gitbucket "$GITBUCKET_REV" "$TWIRL_DIGEST" "$DEPS_CONTENT_DIGEST" classpath || {
  echo "gitbucket dependency classpath is stale or unmarked; run tests/gitbucket_measure.sh once first" >&2
  exit 1
}
JAVA_EXPECTED=$(fixture_cfg gitbucket java_source_count)
if (( ${#JAVA_FILES[@]} != JAVA_EXPECTED )); then
  echo "expected $JAVA_EXPECTED gitbucket Java sources, found ${#JAVA_FILES[@]}" >&2
  exit 1
fi

# --- private work area, one per worktree, locked ----------------------------
ID=${GB_RUN_ID:-$(printf '%s' "$ROOT" | shasum | cut -c1-10)}
WORK=$DIR/w-$ID
mkdir -p "$WORK"
LOCK=$WORK/.lock
LOCK_HELD=0
LOCK_ID=
lock_identity() {
  local target=$1 id
  id=$(stat -c '%d:%i' "$target" 2>/dev/null || true)
  [[ -n $id ]] || id=$(stat -f '%d:%i' "$target" 2>/dev/null || true)
  print -r -- "$id"
}
lock_cleanup() {
  local owner current_id
  (( LOCK_HELD )) || return 0
  current_id=$(lock_identity "$LOCK")
  # The identity check prevents an old process from removing a lock that was
  # reclaimed and replaced after it stopped owning it.
  [[ -n $LOCK_ID && $current_id == $LOCK_ID ]] || return 0
  owner=$(cat "$LOCK/pid" 2>/dev/null || true)
  [[ -z $owner || $owner == $$ ]] || return 0
  fixture_safe_clean "$WORK" "$LOCK" >/dev/null 2>&1 || true
  LOCK_HELD=0
}
lock_signal_exit() {
  local signal=$1 exit_code
  case $signal in
    INT) exit_code=130 ;;
    TERM) exit_code=143 ;;
    *) exit_code=1 ;;
  esac
  trap - INT TERM
  exit $exit_code
}
trap lock_cleanup EXIT
trap 'lock_signal_exit INT' INT
trap 'lock_signal_exit TERM' TERM

# mkdir is the atomic claim.  Publish the owner only after the complete PID
# file has been written, so another process can never observe an empty pid.
while ! mkdir "$LOCK" 2>/dev/null; do
  OWNER=$(cat "$LOCK/pid" 2>/dev/null || true)
  if [[ -z $OWNER ]]; then
    echo "another gitbucket_run.sh is initializing $WORK; try again later." >&2
    exit 1
  fi
  if [[ $OWNER != <-> ]] || kill -0 "$OWNER" 2>/dev/null; then
    echo "another gitbucket_run.sh (pid $OWNER) is using $WORK." >&2
    echo "wait for it, or run with GB_RUN_ID=<something else>." >&2
    exit 1
  fi
  # Move a dead owner's directory out of the way atomically.  If another
  # claimant wins the race, the next loop observes its live PID and exits.
  STALE=$WORK/.lock.stale.$$.$RANDOM
  if mv "$LOCK" "$STALE" 2>/dev/null; then
    fixture_safe_clean "$WORK" "$STALE" >/dev/null 2>&1 || true
  fi
done
LOCK_HELD=1
LOCK_ID=$(lock_identity "$LOCK")
LOCK_PID_TMP=$LOCK/.pid.$$
if ! print -r -- $$ > "$LOCK_PID_TMP" 2>/dev/null ||
   ! mv -f -- "$LOCK_PID_TMP" "$LOCK/pid" 2>/dev/null; then
  echo "could not record gitbucket_run.sh lock owner for $WORK." >&2
  exit 1
fi

# gitbucket's three Java helpers, compiled first as its mixed sbt build does.
JAVA_OUT=$WORK/java
fixture_safe_clean "$WORK" "$JAVA_OUT"; mkdir -p "$JAVA_OUT"
DEPS=$(cat "$SP/deps.cp")
fixture_validate_classpath "$DEPS:$REFLECT:$LIB"
"$JAVAC_BIN" -cp "$DEPS:$LIB" -d "$JAVA_OUT" "${JAVA_FILES[@]}" > "$WORK/javac.log" 2>&1 \
  || { cat "$WORK/javac.log"; echo "gitbucket Java compilation failed" >&2; exit 1; }
CP_DEPS=$JAVA_OUT:$DEPS:$REFLECT
KEY_CP="$CP_DEPS:$LIB"
fixture_validate_classpath "$KEY_CP"
SOURCE_DIGEST=$(fixture_source_digest "$FIXTURE_ROOT" "${FILES[@]}" "${JAVA_FILES[@]}")
RS_FLAGS="-Xsource:3-cross -language:postfixOps --scala-library $SCALA_VERSION"
RS_FLAGS+=" args_sha256=$(fixture_argument_vector_digest)"
BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
if [[ -z ${SCALA_RS:-} ]]; then
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>$WORK/build.log \
    || { cat "$WORK/build.log"; exit 1; }
fi
RS_KEY=$(fixture_artifact_key gitbucket scala-rs "$BIN" "$GITBUCKET_REV" "$SOURCE_DIGEST" "$RS_FLAGS" "$KEY_CP")
RS_DIR=$(fixture_cache_path gitbucket scala-rs "$RS_KEY")
SCALAC_FLAGS="-Xsource:3-cross -language:postfixOps -Wconf:cat=scala3-migration:s"
SCALAC_KEY=$(fixture_artifact_key gitbucket scalac "$SCALAC" "$GITBUCKET_REV" "$SOURCE_DIGEST" "$SCALAC_FLAGS" "$KEY_CP")
SCALAC_DIR=$(fixture_cache_path gitbucket scalac "$SCALAC_KEY")
SCALAC_CACHE_HIT=0

# --- (b) reference build: real scalac. Shared and reused. -------------------
# gitbucket's own `scalacOptions` minus the warning settings and the optimiser,
# the same set `gitbucket_measure.sh` passes us. `-Wconf:cat=scala3-migration:s`
# is added because `-Xsource:3-cross` turns nsc's migration advice into errors
# and gitbucket's build has its own `-Wconf` for that; it changes nothing that
# is emitted.
if [[ ${REUSE_SCALAC:-1} == 1 ]] && fixture_artifact_valid "$SCALAC_DIR" "$SCALAC_KEY" "${#FILES[@]}" gitbucket scalac "$SCALAC" "$GITBUCKET_REV" "$SOURCE_DIGEST" "$SCALAC_FLAGS" "$KEY_CP"; then
  SCALAC_CACHE_HIT=1
else
  echo "== compiling gitbucket with real scalac (once) =="
  STAGE_SCALAC=$(fixture_artifact_stage gitbucket scalac "$SCALAC_KEY")
  mkdir -p "$STAGE_SCALAC/out"
  "$SCALAC" "${FILES[@]}" -d "$STAGE_SCALAC/out" -cp "$CP_DEPS" \
    -Xsource:3-cross -language:postfixOps -Wconf:cat=scala3-migration:s \
    > "$WORK/scalac.log" 2>&1 || { fixture_safe_clean "$FIXTURE_CACHE" "$STAGE_SCALAC"; echo "real scalac failed; see $WORK/scalac.log" >&2; exit 1; }
  fixture_safe_clean "$WORK" "$WORK/out-scalac"
  cp -R "$STAGE_SCALAC/out" "$WORK/out-scalac"
  fixture_artifact_publish "$STAGE_SCALAC" "$SCALAC_DIR" "$SCALAC_KEY" \
    "fixture=gitbucket" "kind=scalac" "source_count=${#FILES[@]}" "compiler_hash=$(fixture_file_hash "$SCALAC")" \
    "upstream_revision=$GITBUCKET_REV" "source_digest=$SOURCE_DIGEST" \
    "flags=$SCALAC_FLAGS" "classpath_digest=$(fixture_classpath_digest "$KEY_CP")" \
    "scala_toolchain=$(fixture_toolchain_digest)" \
    "jdk=$(fixture_jdk_fingerprint)" "scalac=$SCALAC" \
    "scalac_hash=$(fixture_file_hash "$SCALAC")"
fi

# --- (a) build under test: scala-rs -----------------------------------------
# Same inputs and same flags as `tests/gitbucket_measure.sh`.
RS_CACHE_HIT=0
if [[ ${REUSE_RS:-1} == 1 ]] && fixture_artifact_valid "$RS_DIR" "$RS_KEY" "${#FILES[@]}" gitbucket scala-rs "$BIN" "$GITBUCKET_REV" "$SOURCE_DIGEST" "$RS_FLAGS" "$KEY_CP"; then
  RS_CACHE_HIT=1
else
  echo "== compiling gitbucket with scala-rs =="
  fixture_safe_clean "$WORK" "$WORK/out-rs"; mkdir -p "$WORK/out-rs"
  COMPILER_EXIT=0
  "$BIN" compile "${FILES[@]}" -d "$WORK/out-rs" -cp "$CP_DEPS" \
    -Xsource:3-cross -language:postfixOps --scala-library "$LIB" \
    > "$WORK/rs.log" 2>&1 || COMPILER_EXIT=$?
  E=$(grep -c '^error' "$WORK/rs.log" || true)
  C=$(find "$WORK/out-rs" -name '*.class' | wc -l | tr -d ' ')
  echo "   scala-rs: files=${#FILES[@]} java=${#JAVA_FILES[@]} errors=$E classes=$C compiler_exit=$COMPILER_EXIT"
  if (( COMPILER_EXIT != 0 || E != 0 || C == 0 || ${#FILES[@]} == 0 )); then
    echo "gitbucket compile failed; see $WORK/rs.log" >&2
    exit 1
  fi
  STAGE_RS=$(fixture_artifact_stage gitbucket scala-rs "$RS_KEY")
  cp -R "$WORK/out-rs" "$STAGE_RS/out"
  fixture_artifact_publish "$STAGE_RS" "$RS_DIR" "$RS_KEY" \
    "fixture=gitbucket" "kind=scala-rs" "source_count=${#FILES[@]}" "compiler_hash=$(fixture_file_hash "$BIN")" \
    "upstream_revision=$GITBUCKET_REV" "source_digest=$SOURCE_DIGEST" \
    "flags=$RS_FLAGS" "classpath_digest=$(fixture_classpath_digest "$KEY_CP")" \
    "scala_toolchain=$(fixture_toolchain_digest)" \
    "jdk=$(fixture_jdk_fingerprint)" "scalac=$SCALAC" \
    "scalac_hash=$(fixture_file_hash "$SCALAC")"
fi
if (( RS_CACHE_HIT )); then
  RS_OUT=$RS_DIR/out
else
  RS_OUT=$WORK/out-rs
fi
if (( SCALAC_CACHE_HIT )); then
  SCALAC_OUT=$SCALAC_DIR/out
else
  SCALAC_OUT=$WORK/out-scalac
fi
RS_CLASSES=$(find "$RS_OUT" -name '*.class' | wc -l | tr -d ' ')

# Structural check over *every* class, not only the ones the programs reach: a
# branch offset that wrapped, or a method over the 64 KB JVMS 4.7.3 allows.
# Invisible to the loader and to the runs below.
LINT=$(LINT_JOBS=${GBRUN_JOBS:-${LINT_JOBS:-4}} python3 "$ROOT/tests/classfile_lint.py" "$RS_OUT" | tail -1)
echo "   lint: $LINT"

find_sorted "$ROOT/tests/gbrun/support" -name '*.scala'
SUPPORT=("${FIND_RESULT[@]}")
CP_A=$RS_OUT:$RES:$CP_DEPS:$LIB
CP_B=$SCALAC_OUT:$RES:$CP_DEPS:$LIB
CLIENT_OPTS=(-Xsource:3-cross -language:postfixOps)


# --- expected failures ------------------------------------------------------
# Programs that do NOT pass yet, each with the root that stops it. The list is a
# ledger, not a mute button, and it is checked in BOTH directions: an unlisted
# failure fails this script, and a *listed* program that passes fails it too, so
# the list has to shrink as the roots are fixed. Every entry is printed on every
# run, so a skipped check can never read as a passing one.
#
# Pass `KNOWN=''` to hold nothing out.
typeset -A KNOWN_WHY
KNOWN_WHY=()
if [[ -n ${KNOWN:-} ]]; then
  for p in "${(@s: :)KNOWN}"; do KNOWN_WHY[$p]="held out by KNOWN="; done
fi
# Empty since `agent/rhfix`: `Utils` passes on both axes. Two roots were in its
# way -- `returning … insert` inferring the id as `Nothing`, and `Byte` pickled
# as a root-owned reference. `crates/cli/tests/rhf.rs` holds the reduced
# `Utils` programs.

PROGS=("$@")
if [[ ${#PROGS[@]} -eq 0 ]]; then
  PROGS=("$ROOT"/tests/gbrun/*.scala(N:t:r))
fi

PASS=0; DIFF=0; FAIL=0; PICKLE_FAIL=0
UNEXPECTED_PASS=()
for p in "${PROGS[@]}"; do
  src=$ROOT/tests/gbrun/$p.scala
  out=$WORK/progs/$p
  fixture_safe_clean "$WORK/progs" "$out"; mkdir -p "$out/b" "$out/a"
  # --- codegen axis: one binary, two libraries ---
  if ! "$SCALAC" "$src" "${SUPPORT[@]}" -d "$out/b" -cp "$CP_B" "${CLIENT_OPTS[@]}" \
       > "$out/compile-b.log" 2>&1; then
    echo "COMPILE-FAIL-B $p   (see $out/compile-b.log)"; FAIL=$((FAIL+1)); continue
  fi
  rb=0; "$JAVA_BIN" -Xverify:all -cp "$out/b:$CP_B" Main > "$out/b.out" 2> "$out/b.err" || rb=$?
  ra=0; "$JAVA_BIN" -Xverify:all -cp "$out/b:$CP_A" Main > "$out/a.out" 2> "$out/a.err" || ra=$?
  if (( rb != 0 )); then
    echo "REF-FAIL     $p   scalac-built gitbucket could not run it (see $out/b.err)"
    FAIL=$((FAIL+1)); continue
  fi
  if (( ra != 0 )); then
    echo "RUN-FAIL     $p   rs=$ra  (see $out/a.err)"; FAIL=$((FAIL+1)); continue
  fi
  if ! cmp -s "$out/a.out" "$out/b.out"; then
    echo "DIFF         $p   (diff $out/b.out $out/a.out)"; DIFF=$((DIFF+1)); continue
  fi
  # --- pickle axis: compile the same client against OUR classes ---
  if [[ ${PICKLE:-1} == 1 ]]; then
    if ! "$SCALAC" "$src" "${SUPPORT[@]}" -d "$out/a" -cp "$CP_A" "${CLIENT_OPTS[@]}" \
         > "$out/compile-a.log" 2>&1; then
      echo "PICKLE-FAIL  $p   scalac cannot compile against our gitbucket (see $out/compile-a.log)"
      PICKLE_FAIL=$((PICKLE_FAIL+1)); FAIL=$((FAIL+1)); continue
    fi
    rp=0; "$JAVA_BIN" -Xverify:all -cp "$out/a:$CP_A" Main > "$out/p.out" 2> "$out/p.err" || rp=$?
    if (( rp != 0 )); then
      echo "RUN-FAIL-P   $p   rs=$rp  (client built against our pickles; see $out/p.err)"
      FAIL=$((FAIL+1)); continue
    fi
    if ! cmp -s "$out/p.out" "$out/b.out"; then
      echo "DIFF-P       $p   (diff $out/b.out $out/p.out)"; DIFF=$((DIFF+1)); continue
    fi
  fi
  echo "ok           $p"
  PASS=$((PASS+1))
  [[ -n ${KNOWN_WHY[$p]:-} ]] && UNEXPECTED_PASS+=("$p")
done

# Account for the expected failures, in both directions.
for p in "${PROGS[@]}"; do
  [[ -n ${KNOWN_WHY[$p]:-} ]] || continue
  print -- "   known-fail $p: ${KNOWN_WHY[$p]}"
done
NEW=$(( FAIL + DIFF ))
for p in "${PROGS[@]}"; do
  [[ -n ${KNOWN_WHY[$p]:-} ]] && NEW=$(( NEW - 1 ))
done
NEW=$(( NEW + ${#UNEXPECTED_PASS[@]} ))
if (( ${#UNEXPECTED_PASS[@]} )); then
  print -- "   a known-fail now PASSES, so the ledger is stale: ${UNEXPECTED_PASS[*]}"
fi
echo "progs=${#PROGS[@]} ok=$PASS diff=$DIFF fail=$FAIL classes=$RS_CLASSES pickle_fail=$PICKLE_FAIL known_fail=${#KNOWN_WHY[@]} new=$NEW ($LINT, work=$WORK)"
(( NEW <= 0 )) || exit 1
