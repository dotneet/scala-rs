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
#   REUSE_RS=1      do not recompile cats with scala-rs (reuse $WORK/out-rs)
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
SP=/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/cats
DIR=${CATS_RUN_DIR:-/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/catsrun}
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
SCALAC=/tmp/scala-2.13.16/bin/scalac
LIB=/tmp/scala-rs-lib/scala-library-2.13.16.jar
CCACHE=$HOME/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2
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
for j in $KP $BMF; do
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
DIRS=($SP/cats/kernel/src/main/scala $SP/cats/kernel/src/main/scala-2.13+ $KGEN
      $SP/cats/core/src/main/scala $SP/cats/core/src/main/scala-2
      $SP/cats/core/src/main/scala-2.13+ $CGEN)
FILES=($(find $DIRS -name '*.scala' | sort))
DEPS=$(cat $SP/deps.cp)

# --- private work area, one per worktree, locked ----------------------------
ID=${CATS_RUN_ID:-$(printf '%s' "$ROOT" | shasum | cut -c1-10)}
WORK=$DIR/w-$ID
mkdir -p $WORK
LOCK=$WORK/.lock
if ! mkdir $LOCK 2>/dev/null; then
  OWNER=$(cat $LOCK/pid 2>/dev/null || echo '?')
  if [[ $OWNER == '?' ]] || kill -0 $OWNER 2>/dev/null; then
    echo "another cats_run.sh (pid $OWNER) is using $WORK." >&2
    echo "wait for it, or run with CATS_RUN_ID=<something else>." >&2
    exit 1
  fi
  echo "note: taking over the lock of dead pid $OWNER" >&2
fi
echo $$ > $LOCK/pid
trap 'rm -rf $LOCK' EXIT INT TERM

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
if [[ ${REUSE_SCALAC:-1} != 1 || ! -d $DIR/out-scalac ]]; then
  echo "== compiling cats with real scalac (once) =="
  rm -rf $WORK/out-scalac; mkdir -p $WORK/out-scalac
  $SCALAC "${FILES[@]}" -d $WORK/out-scalac -cp "$DEPS" -Xsource:3 \
    -Wconf:cat=scala3-migration:s -language:experimental.macros \
    -no-specialization -Xplugin:$KP -Xplugin:$BMF \
    > $WORK/scalac.log 2>&1 || { echo "real scalac failed; see $WORK/scalac.log" >&2; exit 1; }
  if [[ -d $DIR/out-scalac && ${REUSE_SCALAC:-1} == 1 ]]; then
    rm -rf $WORK/out-scalac        # somebody else published one while we built
  else
    OLD=$DIR/out-scalac.old.$$
    mv $DIR/out-scalac $OLD 2>/dev/null || true
    mv $WORK/out-scalac $DIR/out-scalac
    rm -rf $OLD
  fi
fi

# --- (a) build under test: scala-rs -----------------------------------------
# Same inputs and same flags as `tests/cats_measure.sh` with `CATS_EXCLUDE=''`.
if [[ ${REUSE_RS:-0} != 1 || ! -d $WORK/out-rs ]]; then
  echo "== compiling cats with scala-rs =="
  BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
  if [[ -z ${SCALA_RS:-} ]]; then
    (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>$WORK/build.log \
      || { cat $WORK/build.log; exit 1; }
  fi
  rm -rf $WORK/out-rs; mkdir -p $WORK/out-rs
  COMPILER_EXIT=0
  "$BIN" compile "${FILES[@]}" -d $WORK/out-rs -cp "$DEPS" -Xsource:3 \
    -no-specialization -Ykind-projector --scala-library $LIB \
    > $WORK/rs.log 2>&1 || COMPILER_EXIT=$?
  E=$(grep -c '^error' $WORK/rs.log || true)
  C=$(find $WORK/out-rs -name '*.class' | wc -l | tr -d ' ')
  # `files=` alongside `errors=`/`classes=`: a truncated checkout reads as a
  # clean build otherwise (the lesson `cats_measure.sh` learned the hard way).
  echo "   scala-rs: files=${#FILES[@]} errors=$E classes=$C compiler_exit=$COMPILER_EXIT"
  if (( COMPILER_EXIT != 0 || E != 0 || C == 0 || ${#FILES[@]} == 0 )); then
    echo "cats compile failed; see $WORK/rs.log" >&2
    exit 1
  fi
fi
RS_CLASSES=$(find $WORK/out-rs -name '*.class' | wc -l | tr -d ' ')

# Structural check over *every* class, not only the ones the programs below
# reach: a branch offset that wrapped, or a method over the 64 KB JVMS 4.7.3
# allows. Invisible to the loader (which stops after the constant pool) and to
# the runs below (which touch a fraction of the methods).
LINT=$(LINT_JOBS=${CATSRUN_JOBS:-${LINT_JOBS:-4}} python3 "$ROOT/tests/classfile_lint.py" $WORK/out-rs | tail -1)
echo "   lint: $LINT"

CP_A=$WORK/out-rs:$DEPS:$LIB
CP_B=$DIR/out-scalac:$DEPS:$LIB
# The clients use `F[A, *]` type lambdas -- the way cats is written and the way
# cats is used. Both client compiles are real scalac, so the plugin is on both
# sides and biases neither.
CLIENT_OPTS=(-Xplugin:$KP)


# --- expected failures ------------------------------------------------------
# Programs that do NOT pass yet, each with the root that stops it. The list is a
# ledger, not a mute button, and it is checked in BOTH directions: an unlisted
# failure fails this script, and a *listed* program that passes fails it too, so
# the list has to shrink as the roots are fixed. Every entry is printed on every
# run, so a skipped check can never read as a passing one.
#
# Pass `KNOWN=''` to hold nothing out.
typeset -A KNOWN_WHY
if [[ -n ${KNOWN+x} ]]; then
  for p in ${=KNOWN}; do KNOWN_WHY[$p]="held out by KNOWN="; done
else
  # Empty since `agent/rhfix`: `Chains`, `Monoids`, `NaturalTransforms` and
  # `Transformers` all pass. What they were waiting for is in
  # `docs/notes/rh-cats-gitbucket-run.md`, each with its reduced program.
  KNOWN_WHY=()
fi

PROGS=($@)
if [[ ${#PROGS[@]} -eq 0 ]]; then
  PROGS=($(cd $ROOT/tests/catsrun && ls *.scala | sed 's/\.scala$//' | sort))
fi

PASS=0; DIFF=0; FAIL=0; PICKLE_FAIL=0
KNOWN_HIT=(); UNEXPECTED_PASS=()
for p in $PROGS; do
  src=$ROOT/tests/catsrun/$p.scala
  out=$WORK/progs/$p
  rm -rf $out; mkdir -p $out/b $out/a
  # --- codegen axis: one binary, two libraries ---
  if ! $SCALAC $src -d $out/b -cp "$CP_B" "${CLIENT_OPTS[@]}" > $out/compile-b.log 2>&1; then
    echo "COMPILE-FAIL-B $p   (see $out/compile-b.log)"; FAIL=$((FAIL+1)); continue
  fi
  rb=0; java -Xverify:all -cp $out/b:$CP_B Main > $out/b.out 2> $out/b.err || rb=$?
  ra=0; java -Xverify:all -cp $out/b:$CP_A Main > $out/a.out 2> $out/a.err || ra=$?
  if (( rb != 0 )); then
    echo "REF-FAIL     $p   scalac-built cats could not run it (see $out/b.err)"
    FAIL=$((FAIL+1)); continue
  fi
  if (( ra != 0 )); then
    echo "RUN-FAIL     $p   rs=$ra  (see $out/a.err)"; FAIL=$((FAIL+1)); continue
  fi
  if ! cmp -s $out/a.out $out/b.out; then
    echo "DIFF         $p   (diff $out/b.out $out/a.out)"; DIFF=$((DIFF+1)); continue
  fi
  # --- pickle axis: compile the same client against OUR classes ---
  if [[ ${PICKLE:-1} == 1 ]]; then
    if ! $SCALAC $src -d $out/a -cp "$CP_A" "${CLIENT_OPTS[@]}" > $out/compile-a.log 2>&1; then
      echo "PICKLE-FAIL  $p   scalac cannot compile against our cats (see $out/compile-a.log)"
      PICKLE_FAIL=$((PICKLE_FAIL+1)); FAIL=$((FAIL+1)); continue
    fi
    rp=0; java -Xverify:all -cp $out/a:$CP_A Main > $out/p.out 2> $out/p.err || rp=$?
    if (( rp != 0 )); then
      echo "RUN-FAIL-P   $p   rs=$rp  (client built against our pickles; see $out/p.err)"
      FAIL=$((FAIL+1)); continue
    fi
    if ! cmp -s $out/p.out $out/b.out; then
      echo "DIFF-P       $p   (diff $out/b.out $out/p.out)"; DIFF=$((DIFF+1)); continue
    fi
  fi
  echo "ok           $p"
  PASS=$((PASS+1))
  [[ -n ${KNOWN_WHY[$p]:-} ]] && UNEXPECTED_PASS+=($p)
done

# Account for the expected failures, in both directions.
for p in $PROGS; do
  [[ -n ${KNOWN_WHY[$p]:-} ]] || continue
  print -- "   known-fail $p: ${KNOWN_WHY[$p]}"
done
NEW=$(( FAIL + DIFF ))
for p in $PROGS; do
  [[ -n ${KNOWN_WHY[$p]:-} ]] && NEW=$(( NEW - 1 ))
done
NEW=$(( NEW + ${#UNEXPECTED_PASS[@]} ))
if (( ${#UNEXPECTED_PASS[@]} )); then
  print -- "   a known-fail now PASSES, so the ledger is stale: ${UNEXPECTED_PASS[*]}"
fi
echo "progs=${#PROGS[@]} ok=$PASS diff=$DIFF fail=$FAIL classes=$RS_CLASSES pickle_fail=$PICKLE_FAIL known_fail=${#KNOWN_WHY[@]} new=$NEW ($LINT, work=$WORK)"
(( NEW <= 0 )) || exit 1
