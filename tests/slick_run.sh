#!/bin/zsh
# Differential *execution* test for slick.
#
# slick_measure.sh answers "does scala-rs accept slick?" and slick_subset.sh
# answers "does the JVM verifier accept what came out?".  Neither one runs a
# single instruction of slick.  This script does: it compiles slick twice
# (once with scala-rs, once with real scalac), compiles a set of ordinary
# slick client programs ONCE with real scalac, and runs that one set of client
# classfiles against each of the two slick builds, comparing stdout byte for
# byte.  The client binary is identical in both runs, so any difference is
# caused by slick's classfiles -- i.e. by scala-rs.
#
# Usage: tests/slick_run.sh [prog-name ...]
#   with no arguments every program in tests/slick_progs/ is run.
# Env:
#   SLICK_RUN_DIR   work dir (default: <fixture root>/slickrun)
#   SLICK_RUN_ID    name of the private area inside it (default: a hash of
#                   $ROOT, so every worktree gets its own)
#   RUNS=n          execute each program n times per side (default 3).  A
#                   program counts as ok only if all n attempts pass; the
#                   per-program "m/n" is printed, so a retry can never hide a
#                   failure -- it only tells you the failure is intermittent.
#   REUSE_RS=1      reuse the manifest-validated scala-rs artifact when present
#   REUSE_SCALAC=0  force recompiling slick with real scalac (slow, ~4 min)
#   MODE=b          which slick sits on the *compile* classpath of the client
#                   programs: "b" (default) = the scalac-built slick, so the
#                   client binary is beyond suspicion; "a" = the scala-rs-built
#                   slick, which additionally makes real scalac read scala-rs's
#                   ScalaSignature pickles and run scala-rs's macro classfiles.
#
# On concurrency.  Until 2026-09-05 everything above lived in one directory
# shared by every worktree on the machine, and two overlapping runs silently
# destroyed each other's inputs: each one does `rm -rf $DIR/progs/$p` and then
# `java -cp $DIR/progs/$p ...`, so the loser's JVM cannot find Main, exits 1,
# and the winner's `a.out`/`a.err` are what you are left looking at -- stdout
# byte-identical, stderr three SLF4J lines, exit code 1, no exception.  That is
# a harness bug that reads exactly like a compiler bug.  Hence: the reference
# build (out-scalac) is shared because it depends only on the slick checkout
# and real scalac, and is published by an atomic rename; everything that
# depends on *your* compiler or gets rewritten per run lives under $WORK, which
# is private to your worktree and held under a lock for the duration.
set -eo pipefail
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
source "$ROOT/tests/fixture_cache.sh"
SP=${SLICK_FIXTURE_DIR:-$(fixture_path slick)}
typeset -a FIND_RESULT
find_sorted() {
  FIND_RESULT=( "${(@0)$(find "$@" -print0 | sort -z)}" )
  FIND_RESULT=( "${(@)FIND_RESULT:#}" )
}
DIR=${SLICK_RUN_DIR:-$FIXTURE_ROOT/slickrun}
SCALAC=$(fixture_scalac_path)
REFLECT=$(fixture_toolchain_reflect)
LIB=$(fixture_toolchain_library)
JAVA_BIN=$(fixture_java_path)
CCACHE=$HOME/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2
H2=$CCACHE/com/h2database/h2/2.1.214/h2-2.1.214.jar
SLICK_REV=$(fixture_cfg slick revision)
SCALA_VERSION=$(fixture_toolchain_version)
RUNS=${RUNS:-3}
MODE=${MODE:-b}
if [[ $RUNS != <-> ]] || (( RUNS < 1 )); then
  echo "RUNS must be a positive integer, got: $RUNS" >&2
  exit 2
fi
case $MODE in
  a|b) ;;
  *) echo "MODE must be a or b, got: $MODE" >&2; exit 2 ;;
esac

if [[ ! -x $SCALAC || ! -s $LIB || ! -d $SP/slick/.git || ! -s $SP/deps.cp ]]; then
  echo "toolchain or slick checkout missing; run tests/slick_measure.sh once first (it self-restores)" >&2
  exit 1
fi
fixture_require_checkout slick "$SP/slick"
[[ -s $H2 ]] || { echo "H2 jar not in the Coursier cache: $H2" >&2; exit 1; }

SRC=$SP/slick/slick/src/main
COMPAT=$SP/slick/slick-compat-collections/src/main/scala-2.13+
RES=$SRC/resources
DEPS=$(cat "$SP/deps.cp"):$REFLECT
DEPS_MARK=$SP/.fixture-deps
DEPS_CONTENT_DIGEST=$(fixture_dependency_digest "$SP/deps.cp")
fixture_state_valid "$DEPS_MARK" slick "$SLICK_REV" "$SLICK_REV" "$DEPS_CONTENT_DIGEST" classpath || {
  echo "slick dependency classpath is stale or unmarked; run tests/slick_measure.sh once first" >&2
  exit 1
}
fixture_validate_classpath "$DEPS"

# --- private work area, one per worktree, locked ----------------------------
ID=${SLICK_RUN_ID:-$(printf '%s' "$ROOT" | shasum | cut -c1-10)}
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
    echo "another slick_run.sh is initializing $WORK; try again later." >&2
    exit 1
  fi
  if [[ $OWNER != <-> ]] || kill -0 "$OWNER" 2>/dev/null; then
    echo "another slick_run.sh (pid $OWNER) is using $WORK." >&2
    echo "wait for it, or run with SLICK_RUN_ID=<something else>." >&2
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
  echo "could not record slick_run.sh lock owner for $WORK." >&2
  exit 1
fi

GEN=$WORK/generated
fixture_safe_clean "$WORK" "$GEN"
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

# --- (b) reference build: real scalac. Slow; shared and reused. -------------
# It depends only on the slick checkout and on real scalac, so every worktree
# can use the same copy.  Built into a private directory and published with a
# rename, so a concurrent reader never sees a half-written build.
SOURCE_DIGEST=$(fixture_source_digest "$FIXTURE_ROOT" "${FILES[@]}")
RS_FLAGS="-Xsource:3 --scala-library $SCALA_VERSION"
RS_FLAGS+=" args_sha256=$(fixture_argument_vector_digest)"
RS_CP="$DEPS:$LIB"
BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
if [[ -z ${SCALA_RS:-} ]]; then
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>$WORK/build.log \
    || { cat "$WORK/build.log"; exit 1; }
fi
RS_KEY=$(fixture_artifact_key slick scala-rs "$BIN" "$SLICK_REV" "$SOURCE_DIGEST" "$RS_FLAGS" "$RS_CP")
RS_DIR=$(fixture_cache_path slick scala-rs "$RS_KEY")
SCALAC_FLAGS="-Xsource:3-cross"
SCALAC_CP="$DEPS:$LIB"
SCALAC_KEY=$(fixture_artifact_key slick scalac "$SCALAC" "$SLICK_REV" "$SOURCE_DIGEST" "$SCALAC_FLAGS" "$SCALAC_CP")
SCALAC_DIR=$(fixture_cache_path slick scalac "$SCALAC_KEY")
SCALAC_CACHE_HIT=0
if [[ ${REUSE_SCALAC:-1} == 1 ]] && fixture_artifact_valid "$SCALAC_DIR" "$SCALAC_KEY" "${#FILES[@]}" slick scalac "$SCALAC" "$SLICK_REV" "$SOURCE_DIGEST" "$SCALAC_FLAGS" "$SCALAC_CP"; then
  SCALAC_CACHE_HIT=1
else
  echo "== compiling slick with real scalac (slow, once) =="
  STAGE_SCALAC=$(fixture_artifact_stage slick scalac "$SCALAC_KEY")
  mkdir -p "$STAGE_SCALAC/out"
  "$SCALAC" "${FILES[@]}" -d "$STAGE_SCALAC/out" -cp "$DEPS" -Xsource:3-cross \
    > "$WORK/scalac.log" 2>&1 || { fixture_safe_clean "$FIXTURE_CACHE" "$STAGE_SCALAC"; echo "real scalac failed; see $WORK/scalac.log" >&2; exit 1; }
  fixture_safe_clean "$WORK" "$WORK/out-scalac"
  cp -R "$STAGE_SCALAC/out" "$WORK/out-scalac"
  fixture_artifact_publish "$STAGE_SCALAC" "$SCALAC_DIR" "$SCALAC_KEY" \
    "fixture=slick" "kind=scalac" "source_count=${#FILES[@]}" "compiler_hash=$(fixture_file_hash "$SCALAC")" \
    "upstream_revision=$SLICK_REV" "source_digest=$SOURCE_DIGEST" \
    "flags=$SCALAC_FLAGS" "classpath_digest=$(fixture_classpath_digest "$SCALAC_CP")" \
    "scala_toolchain=$(fixture_toolchain_digest)" \
    "jdk=$(fixture_jdk_fingerprint)" "scalac=$SCALAC" \
    "scalac_hash=$(fixture_file_hash "$SCALAC")"
fi

# --- (a) build under test: scala-rs -----------------------------------------
RS_CACHE_HIT=0
if [[ ${REUSE_RS:-1} == 1 ]] && fixture_artifact_valid "$RS_DIR" "$RS_KEY" "${#FILES[@]}" slick scala-rs "$BIN" "$SLICK_REV" "$SOURCE_DIGEST" "$RS_FLAGS" "$RS_CP"; then
  RS_CACHE_HIT=1
else
  echo "== compiling slick with scala-rs =="
  fixture_safe_clean "$WORK" "$WORK/out-rs"; mkdir -p "$WORK/out-rs"
  COMPILER_EXIT=0
  "$BIN" compile "${FILES[@]}" -d "$WORK/out-rs" -cp "$DEPS" \
    -Xsource:3 --scala-library "$LIB" > "$WORK/rs.log" 2>&1 || COMPILER_EXIT=$?
  E=$(grep -c '^error' "$WORK/rs.log" || true)
  C=$(find "$WORK/out-rs" -name '*.class' | wc -l | tr -d ' ')
  # Print files= alongside errors= and classes=: a truncated slick checkout
  # reads as a clean build otherwise.
  echo "   scala-rs: files=${#FILES[@]} errors=$E classes=$C compiler_exit=$COMPILER_EXIT"
  if (( COMPILER_EXIT != 0 || E != 0 || C == 0 || ${#FILES[@]} == 0 )); then
    echo "slick compile failed; see $WORK/rs.log" >&2
    exit 1
  fi
  STAGE_RS=$(fixture_artifact_stage slick scala-rs "$RS_KEY")
  cp -R "$WORK/out-rs" "$STAGE_RS/out"
  fixture_artifact_publish "$STAGE_RS" "$RS_DIR" "$RS_KEY" \
    "fixture=slick" "kind=scala-rs" "source_count=${#FILES[@]}" "compiler_hash=$(fixture_file_hash "$BIN")" \
    "upstream_revision=$SLICK_REV" "source_digest=$SOURCE_DIGEST" \
    "flags=$RS_FLAGS" "classpath_digest=$(fixture_classpath_digest "$RS_CP")" \
    "scala_toolchain=$(fixture_toolchain_digest)" \
    "jdk=$(fixture_jdk_fingerprint)" "scalac=$SCALAC" \
    "scalac_hash=$(fixture_file_hash "$SCALAC")"
  # Structural check over *every* class we just wrote, not only the ones the
  # programs below happen to call: a branch offset that wrapped, or a method
  # over the 64 KB the format allows. Neither is visible to the loader check
  # in slick_subset.sh (which stops after the constant pool) nor to the runs
  # below (which reach a fraction of the methods). ~3 s for 1600 classes.
  python3 "$ROOT/tests/classfile_lint.py" "$WORK/out-rs" | tail -20
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
CP_A=$RS_OUT:$RES:$DEPS:$H2:$LIB
CP_B=$SCALAC_OUT:$RES:$DEPS:$H2:$LIB
[[ $MODE == a ]] && CP_COMPILE=$CP_A || CP_COMPILE=$CP_B

PROGS=("$@")
if [[ ${#PROGS[@]} -eq 0 ]]; then
  PROGS=("$ROOT"/tests/slick_progs/*.scala(N:t:r))
fi

load() { uptime | sed 's/.*load averages*: //'; }

# Both directions may reuse the same library outputs, but their client
# classfiles and execution evidence must survive running the other direction.
PROG_DIR=$WORK/progs-$MODE
mkdir -p "$PROG_DIR"
PASS=0; DIFF=0; FAIL=0; ATT=0; ATT_OK=0
for p in "${PROGS[@]}"; do
  src=$ROOT/tests/slick_progs/$p.scala
  out=$PROG_DIR/$p
  fixture_safe_clean "$PROG_DIR" "$out"; mkdir -p "$out"
  if ! "$SCALAC" "$src" -d "$out" -cp "$CP_COMPILE" > "$out/compile.log" 2>&1; then
    echo "COMPILE-FAIL $p   (see $out/compile.log)"; FAIL=$((FAIL+1)); continue
  fi
  # Execute RUNS times.  Every attempt that is not a clean byte-identical pass
  # is reported and its stdout/stderr kept under a per-attempt name, so an
  # intermittent failure is visible instead of being averaged away.
  okc=0; verdict=ok; firstbad=0
  for k in $(seq 1 $RUNS); do
    ATT=$((ATT+1))
    ra=0; "$JAVA_BIN" -cp "$out:$CP_A" Main > "$out/a.out" 2> "$out/a.err" || ra=$?
    rb=0; "$JAVA_BIN" -cp "$out:$CP_B" Main > "$out/b.out" 2> "$out/b.err" || rb=$?
    if [[ $ra != 0 || $rb != 0 ]]; then
      verdict=fail
      echo "   attempt $k/$RUNS $p: rs=$ra scalac=$rb  load=$(load)"
    elif ! cmp -s "$out/a.out" "$out/b.out"; then
      [[ $verdict == fail ]] || verdict=diff
      echo "   attempt $k/$RUNS $p: stdout differs  load=$(load)"
    else
      okc=$((okc+1)); ATT_OK=$((ATT_OK+1)); continue
    fi
    [[ $firstbad != 0 ]] || firstbad=$k
    for f in a.out a.err b.out b.err; do cp "$out/$f" "$out/attempt$k-$f" 2>/dev/null || true; done
  done
  case $verdict in
    ok)   echo "ok           $p   $okc/$RUNS"; PASS=$((PASS+1));;
    diff) echo "DIFF         $p   $okc/$RUNS  (diff $out/attempt$firstbad-b.out $out/attempt$firstbad-a.out)"; DIFF=$((DIFF+1));;
    fail) echo "RUN-FAIL     $p   $okc/$RUNS  (see $out/attempt*-a.err $out/attempt*-b.err)"; FAIL=$((FAIL+1));;
  esac
done
echo "progs=${#PROGS[@]} ok=$PASS diff=$DIFF fail=$FAIL  runs=$RUNS attempts=$ATT_OK/$ATT  (compile-cp=$MODE, work=$WORK)"
[[ $DIFF -eq 0 && $FAIL -eq 0 ]] || exit 1
