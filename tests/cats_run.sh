#!/bin/zsh
# Differential *execution* test for cats.
#
# `cats_measure.sh` answers "does scala-rs accept cats?" and `classfile_lint.py`
# answers "are the classfiles structurally possible?". Neither runs a single
# instruction of cats, and neither can: the first miscompilation this script
# found was a two-argument call emitted with one argument -- a `VerifyError` the
# loader only raises when something asks for the class. The lint pass reported
# `lint_classes=2976 lint_problems=0` on the same build.
#
# So, like `tests/slick_run.sh`: compile cats twice, once with scala-rs and
# once with real scalac, then compile a set of ordinary cats client programs
# with *real scalac* and run them against both builds, comparing stdout byte
# for byte.
#
# Each program is compiled TWICE, which separates the two ways we can be wrong:
#
#   * against scalac's cats ("side B"): the client binary is beyond suspicion,
#     so a difference when it runs against our classes is a **codegen** defect;
#   * against our cats ("side A"): real scalac reads *our* `ScalaSignature`
#     pickles and runs *our* macro classfiles to compile it, so a failure here
#     is a **pickle** defect -- the classes may run perfectly and still be
#     unusable. Both of the first two defects in this slice were of that kind.
#
# A program counts as `ok` only when both compiles succeed, both runs exit 0,
# and all three stdouts (B-binary on B, B-binary on A, A-binary on A) are
# identical. The per-program line says which half failed.
#
# Usage: tests/cats_run.sh [prog-name ...]
#   with no arguments every program in tests/catsrun/ is run.
# Env:
#   CATS_RUN_DIR    work dir (default: a sibling of the cats checkout)
#   CATS_RUN_ID     name of the private area inside it (default: a hash of
#                   $ROOT, so every worktree gets its own)
#   REUSE_RS=1      reuse the manifest-validated scala-rs artifact when present
#   REUSE_SCALAC=0  force recompiling cats with real scalac (~15 s)
#   PICKLE=0        skip the side-A compiles (codegen axis only)
#   SCALA_RS=<path> use this binary instead of building one
#
# On concurrency: the reference build depends only on the cats checkout and on
# real scalac, so it is shared between worktrees and published with an atomic
# rename. Everything that depends on *your* compiler lives under $WORK, which
# is private to your worktree and held under a lock for the duration. See
# `tests/slick_run.sh`'s header for what a shared work directory did the one
# time this was not the case.
set -eo pipefail
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
source "$ROOT/tests/fixture_cache.sh"
SP=${CATS_FIXTURE_DIR:-$(fixture_path cats)}
typeset -a FIND_RESULT
find_sorted() {
  FIND_RESULT=( "${(@0)$(find "$@" -print0 | sort -z)}" )
  FIND_RESULT=( "${(@)FIND_RESULT:#}" )
}
DIR=${CATS_RUN_DIR:-$FIXTURE_ROOT/catsrun}
SCALAC=$(fixture_scalac_path)
LIB=$(fixture_toolchain_library)
JAVA_BIN=$(fixture_java_path)
CCACHE=$HOME/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2
CATS_REV=$(fixture_cfg cats revision)
SCALA_VERSION=$(fixture_toolchain_version)
# cats is built with two compiler plugins. kind-projector is load-bearing for
# *both* sides: `λ[α => …]` and `F[A, *]` are its syntax, and real scalac
# rejects the source without it (we implement it natively, `-Ykind-projector`).
# better-monadic-for only changes how `for` desugars, but cats' build has it, so
# the reference build gets it too.
KP=$CCACHE/org/typelevel/kind-projector_2.13.16/0.13.3/kind-projector_2.13.16-0.13.3.jar
BMF=$CCACHE/com/olegpy/better-monadic-for_2.13/0.3.1/better-monadic-for_2.13-0.3.1.jar

if [[ ! -x $SCALAC || ! -s $LIB || ! -d $SP/cats/.git || ! -s $SP/deps.cp ]]; then
  echo "toolchain or cats checkout missing; run tests/cats_measure.sh once first (it self-restores)" >&2
  exit 1
fi
fixture_require_checkout cats "$SP/cats"
for j in "$KP" "$BMF"; do
  [[ -s $j ]] || { echo "compiler plugin not in the Coursier cache: $j" >&2; exit 1; }
done

# The source set `tests/cats_measure.sh` measures: the cross-version directories
# sbt selects for 2.13, plus the 16 files sbt's source generators write.
KGEN=$SP/cats/kernel/.jvm/target/scala-2.13/src_managed/main
CGEN=$SP/cats/core/.jvm/target/scala-2.13/src_managed/main
if [[ ! -d $KGEN || ! -d $CGEN ]]; then
  echo "cats' generated sources are missing; run tests/cats_measure.sh once first" >&2
  exit 1
fi
find_sorted "$SP/cats/kernel/src/main/scala" "$SP/cats/kernel/src/main/scala-2.13+" \
  "$SP/cats/core/src/main/scala" "$SP/cats/core/src/main/scala-2" \
  "$SP/cats/core/src/main/scala-2.13+" -name '*.scala'
GEN_INPUTS=("${FIND_RESULT[@]}")
GEN_INPUT_DIGEST=$(fixture_source_digest "$FIXTURE_ROOT" "${GEN_INPUTS[@]}")
GEN_EXPECTED=$(fixture_cfg cats generated_count)
GEN_STATE=$(fixture_cfg cats generated_state)
GEN_CONTENT_DIGEST=$(fixture_generated_content_digest "$SP/cats" "$KGEN" "$CGEN")
fixture_generated_valid cats "$SP/cats" "$CATS_REV" "$GEN_INPUT_DIGEST" "$GEN_EXPECTED" "$GEN_STATE" "$GEN_CONTENT_DIGEST" || {
  echo "cats generated sources are stale or unmarked; run tests/cats_measure.sh once first" >&2
  exit 1
}
DIRS=("$SP/cats/kernel/src/main/scala" "$SP/cats/kernel/src/main/scala-2.13+" "$KGEN"
      "$SP/cats/core/src/main/scala" "$SP/cats/core/src/main/scala-2"
      "$SP/cats/core/src/main/scala-2.13+" "$CGEN")
find_sorted "${DIRS[@]}" -name '*.scala'
FILES=("${FIND_RESULT[@]}")
DEPS=$(cat "$SP/deps.cp")
DEPS_MARK=$SP/.fixture-deps
DEPS_CONTENT_DIGEST=$(fixture_dependency_digest "$SP/deps.cp")
fixture_state_valid "$DEPS_MARK" cats "$CATS_REV" "$CATS_REV" "$DEPS_CONTENT_DIGEST" classpath || {
  echo "cats dependency classpath is stale or unmarked; run tests/cats_measure.sh once first" >&2
  exit 1
}
fixture_validate_source_count cats ${#FILES[@]} default
GEN_COUNT=$(find "$KGEN" "$CGEN" -type f -name '*.scala' | wc -l | tr -d ' ')
fixture_validate_generated cats "$GEN_COUNT" "$GEN_STATE"
GEN_CONTENT_DIGEST=$(fixture_generated_content_digest "$SP/cats" "$KGEN" "$CGEN")
fixture_generated_valid cats "$SP/cats" "$CATS_REV" "$GEN_INPUT_DIGEST" "$GEN_COUNT" "$GEN_STATE" "$GEN_CONTENT_DIGEST"
fixture_validate_classpath "$DEPS"

# --- private work area, one per worktree, locked ----------------------------
ID=${CATS_RUN_ID:-$(printf '%s' "$ROOT" | shasum | cut -c1-10)}
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
    echo "another cats_run.sh is initializing $WORK; try again later." >&2
    exit 1
  fi
  if [[ $OWNER != <-> ]] || kill -0 "$OWNER" 2>/dev/null; then
    echo "another cats_run.sh (pid $OWNER) is using $WORK." >&2
    echo "wait for it, or run with CATS_RUN_ID=<something else>." >&2
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
  echo "could not record cats_run.sh lock owner for $WORK." >&2
  exit 1
fi

# --- (b) reference build: real scalac. Shared and reused. -------------------
# `-Wconf:cat=scala3-migration:s` and `-language:experimental.macros` are what
# cats' own build settings amount to here: under `-Xsource:3` nsc reports 37
# migration messages in cats as errors, and `FunctionKMacros.scala` declares a
# `macro` (sbt-typelevel enables the feature). Neither changes what is emitted.
#
# `-no-specialization` is the one flag that is here for *our* sake, and it is
# not cosmetic. cats annotates with `@sp` (`scala.specialized`) all over
# `cats.kernel`, and `tests/cats_measure.sh` passes `-no-specialization` because
# we ignore the annotation -- "which changes the ABI but not what typechecks",
# as its own comment says. Without the same flag here the reference build has
# `Monoid.combine$mcI$sp(int, int)` and ours does not, so a client compiled
# against the reference called a method that exists in only one of the two
# builds: `NoSuchMethodError` on `Monoid[Int].combine(2, 3)`, in every program,
# for a reason that has nothing to do with code generation. Both sides get the
# configuration we actually ship, and the comparison is then about our backend.
SOURCE_DIGEST=$(fixture_source_digest "$SP/cats" "${FILES[@]}")
BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
if [[ -z ${SCALA_RS:-} ]]; then
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>$WORK/build.log \
    || { cat "$WORK/build.log"; exit 1; }
fi
RS_FLAGS="-Xsource:3 -no-specialization -Ykind-projector --scala-library $SCALA_VERSION"
RS_FLAGS+=" args_sha256=$(fixture_argument_vector_digest)"
RS_CP="$DEPS:$LIB"
RS_KEY=$(fixture_artifact_key cats scala-rs "$BIN" "$CATS_REV" "$SOURCE_DIGEST" "$RS_FLAGS" "$RS_CP")
RS_DIR=$(fixture_cache_path cats scala-rs "$RS_KEY")
SCALAC_FLAGS="-Xsource:3 -Wconf:cat=scala3-migration:s -language:experimental.macros -no-specialization -Xplugin:$KP -Xplugin:$BMF"
SCALAC_CP="$DEPS:$KP:$BMF:$LIB"
SCALAC_KEY=$(fixture_artifact_key cats scalac "$SCALAC" "$CATS_REV" "$SOURCE_DIGEST" "$SCALAC_FLAGS" "$SCALAC_CP")
SCALAC_DIR=$(fixture_cache_path cats scalac "$SCALAC_KEY")
SCALAC_CACHE_HIT=0
if [[ ${REUSE_SCALAC:-1} == 1 ]] && fixture_artifact_valid "$SCALAC_DIR" "$SCALAC_KEY" "${#FILES[@]}" cats scalac "$SCALAC" "$CATS_REV" "$SOURCE_DIGEST" "$SCALAC_FLAGS" "$SCALAC_CP"; then
  SCALAC_CACHE_HIT=1
else
  echo "== compiling cats with real scalac (once) =="
  STAGE_SCALAC=$(fixture_artifact_stage cats scalac "$SCALAC_KEY")
  mkdir -p "$STAGE_SCALAC/out"
  "$SCALAC" "${FILES[@]}" -d "$STAGE_SCALAC/out" -cp "$DEPS" -Xsource:3 \
    -Wconf:cat=scala3-migration:s -language:experimental.macros \
    -no-specialization "-Xplugin:$KP" "-Xplugin:$BMF" \
    > "$WORK/scalac.log" 2>&1 || { fixture_safe_clean "$FIXTURE_CACHE" "$STAGE_SCALAC"; echo "real scalac failed; see $WORK/scalac.log" >&2; exit 1; }
  fixture_safe_clean "$WORK" "$WORK/out-scalac"
  cp -R "$STAGE_SCALAC/out" "$WORK/out-scalac"
  fixture_artifact_publish "$STAGE_SCALAC" "$SCALAC_DIR" "$SCALAC_KEY" \
    "fixture=cats" "kind=scalac" "source_count=${#FILES[@]}" "compiler_hash=$(fixture_file_hash "$SCALAC")" \
    "upstream_revision=$CATS_REV" "source_digest=$SOURCE_DIGEST" \
    "flags=$SCALAC_FLAGS" "classpath_digest=$(fixture_classpath_digest "$SCALAC_CP")" \
    "scala_toolchain=$(fixture_toolchain_digest)" \
    "jdk=$(fixture_jdk_fingerprint)" "scalac=$SCALAC" \
    "scalac_hash=$(fixture_file_hash "$SCALAC")"
fi
if (( SCALAC_CACHE_HIT )); then
  SCALAC_OUT=$SCALAC_DIR/out
else
  SCALAC_OUT=$WORK/out-scalac
fi

# --- (a) build under test: scala-rs -----------------------------------------
# Same inputs and same flags as `tests/cats_measure.sh` with `CATS_EXCLUDE=''`.
RS_CACHE_HIT=0
if [[ ${REUSE_RS:-1} == 1 ]] && fixture_artifact_valid "$RS_DIR" "$RS_KEY" "${#FILES[@]}" cats scala-rs "$BIN" "$CATS_REV" "$SOURCE_DIGEST" "$RS_FLAGS" "$RS_CP"; then
  RS_CACHE_HIT=1
else
  echo "== compiling cats with scala-rs =="
  fixture_safe_clean "$WORK" "$WORK/out-rs"; mkdir -p "$WORK/out-rs"
  COMPILER_EXIT=0
  "$BIN" compile "${FILES[@]}" -d "$WORK/out-rs" -cp "$DEPS" -Xsource:3 \
    -no-specialization -Ykind-projector --scala-library "$LIB" \
    > "$WORK/rs.log" 2>&1 || COMPILER_EXIT=$?
  E=$(grep -c '^error' "$WORK/rs.log" || true)
  C=$(find "$WORK/out-rs" -name '*.class' | wc -l | tr -d ' ')
  # `files=` alongside `errors=`/`classes=`: a truncated checkout reads as a
  # clean build otherwise (the lesson `cats_measure.sh` learned the hard way).
  echo "   scala-rs: files=${#FILES[@]} errors=$E classes=$C compiler_exit=$COMPILER_EXIT"
  if (( COMPILER_EXIT != 0 || E != 0 || C == 0 || ${#FILES[@]} == 0 )); then
    echo "cats compile failed; see $WORK/rs.log" >&2
    exit 1
  fi
  STAGE_RS=$(fixture_artifact_stage cats scala-rs "$RS_KEY")
  cp -R "$WORK/out-rs" "$STAGE_RS/out"
  fixture_artifact_publish "$STAGE_RS" "$RS_DIR" "$RS_KEY" \
    "fixture=cats" "kind=scala-rs" "source_count=${#FILES[@]}" "compiler_hash=$(fixture_file_hash "$BIN")" \
    "upstream_revision=$CATS_REV" "source_digest=$SOURCE_DIGEST" \
    "flags=$RS_FLAGS" "classpath_digest=$(fixture_classpath_digest "$RS_CP")" \
    "scala_toolchain=$(fixture_toolchain_digest)" \
    "jdk=$(fixture_jdk_fingerprint)" "scalac=$SCALAC" \
    "scalac_hash=$(fixture_file_hash "$SCALAC")"
fi
if (( RS_CACHE_HIT )); then
  RS_OUT=$RS_DIR/out
else
  RS_OUT=$WORK/out-rs
fi
if [[ ${REUSE_SCALAC:-1} == 1 ]]; then
  fixture_artifact_valid "$SCALAC_DIR" "$SCALAC_KEY" "${#FILES[@]}" cats scalac "$SCALAC" "$CATS_REV" "$SOURCE_DIGEST" "$SCALAC_FLAGS" "$SCALAC_CP" || { echo "scalac cats artifact is incomplete" >&2; exit 1; }
fi
RS_CLASSES=$(find "$RS_OUT" -name '*.class' | wc -l | tr -d ' ')

# Structural check over *every* class, not only the ones the programs below
# reach: a branch offset that wrapped, or a method over the 64 KB JVMS 4.7.3
# allows. Invisible to the loader (which stops after the constant pool) and to
# the runs below (which touch a fraction of the methods).
LINT=$(LINT_JOBS=${CATSRUN_JOBS:-${LINT_JOBS:-4}} python3 "$ROOT/tests/classfile_lint.py" "$RS_OUT" | tail -1)
echo "   lint: $LINT"

CP_A=$RS_OUT:$DEPS:$LIB
CP_B=$SCALAC_OUT:$DEPS:$LIB
# The clients use `F[A, *]` type lambdas -- the way cats is written and the way
# cats is used. Both client compiles are real scalac, so the plugin is on both
# sides and biases neither.
CLIENT_OPTS=("-Xplugin:$KP")


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
# Empty since `agent/rhfix`: `Chains`, `Monoids`, `NaturalTransforms` and
# `Transformers` all pass. What they were waiting for is in
# `docs/notes/rh-cats-gitbucket-run.md`, each with its reduced program.

PROGS=("$@")
if [[ ${#PROGS[@]} -eq 0 ]]; then
  PROGS=("$ROOT"/tests/catsrun/*.scala(N:t:r))
fi

PASS=0; DIFF=0; FAIL=0; PICKLE_FAIL=0
KNOWN_HIT=(); UNEXPECTED_PASS=()
for p in "${PROGS[@]}"; do
  src=$ROOT/tests/catsrun/$p.scala
  out=$WORK/progs/$p
  fixture_safe_clean "$WORK/progs" "$out"; mkdir -p "$out/b" "$out/a"
  # --- codegen axis: one binary, two libraries ---
  if ! "$SCALAC" "$src" -d "$out/b" -cp "$CP_B" "${CLIENT_OPTS[@]}" > "$out/compile-b.log" 2>&1; then
    echo "COMPILE-FAIL-B $p   (see $out/compile-b.log)"; FAIL=$((FAIL+1)); continue
  fi
  rb=0; "$JAVA_BIN" -Xverify:all -cp "$out/b:$CP_B" Main > "$out/b.out" 2> "$out/b.err" || rb=$?
  ra=0; "$JAVA_BIN" -Xverify:all -cp "$out/b:$CP_A" Main > "$out/a.out" 2> "$out/a.err" || ra=$?
  if (( rb != 0 )); then
    echo "REF-FAIL     $p   scalac-built cats could not run it (see $out/b.err)"
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
    if ! "$SCALAC" "$src" -d "$out/a" -cp "$CP_A" "${CLIENT_OPTS[@]}" > "$out/compile-a.log" 2>&1; then
      echo "PICKLE-FAIL  $p   scalac cannot compile against our cats (see $out/compile-a.log)"
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
