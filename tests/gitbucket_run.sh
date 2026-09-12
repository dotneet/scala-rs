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
#   REUSE_RS=1      do not recompile gitbucket with scala-rs (reuse $WORK/out-rs)
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
SP=/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/gitbucket
DIR=${GB_RUN_DIR:-/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/gbrun}
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
SCALAC=/tmp/scala-2.13.16/bin/scalac
REFLECT=/tmp/scala-2.13.16/lib/scala-reflect.jar
LIB=/tmp/scala-rs-lib/scala-library-2.13.16.jar
SRCROOT=$SP/gitbucket
TWIRL=$SRCROOT/target/scala-2.13/twirl/main
RES=$SRCROOT/src/main/resources

if [[ ! -x $SCALAC || ! -s $LIB || ! -d $SRCROOT/.git || ! -s $SP/deps.cp || ! -d $TWIRL ]]; then
  echo "toolchain or gitbucket checkout missing; run tests/gitbucket_measure.sh once first (it self-restores)" >&2
  exit 1
fi

FILES=($(find $SRCROOT/src/main/scala $TWIRL -name '*.scala' | sort))
JAVA_FILES=($SRCROOT/src/main/java/**/*.java(N))
if (( ${#JAVA_FILES[@]} != 3 )); then
  echo "expected 3 gitbucket Java sources, found ${#JAVA_FILES[@]}" >&2
  exit 1
fi

# --- private work area, one per worktree, locked ----------------------------
ID=${GB_RUN_ID:-$(printf '%s' "$ROOT" | shasum | cut -c1-10)}
WORK=$DIR/w-$ID
mkdir -p $WORK
LOCK=$WORK/.lock
if ! mkdir $LOCK 2>/dev/null; then
  OWNER=$(cat $LOCK/pid 2>/dev/null || echo '?')
  if [[ $OWNER == '?' ]] || kill -0 $OWNER 2>/dev/null; then
    echo "another gitbucket_run.sh (pid $OWNER) is using $WORK." >&2
    echo "wait for it, or run with GB_RUN_ID=<something else>." >&2
    exit 1
  fi
  echo "note: taking over the lock of dead pid $OWNER" >&2
fi
echo $$ > $LOCK/pid
trap 'rm -rf $LOCK' EXIT INT TERM

# gitbucket's three Java helpers, compiled first as its mixed sbt build does.
JAVA_OUT=$WORK/java
rm -rf $JAVA_OUT; mkdir -p $JAVA_OUT
DEPS=$(cat $SP/deps.cp)
javac -cp "$DEPS:$LIB" -d $JAVA_OUT "${JAVA_FILES[@]}" > $WORK/javac.log 2>&1 \
  || { cat $WORK/javac.log; echo "gitbucket Java compilation failed" >&2; exit 1; }
CP_DEPS=$JAVA_OUT:$DEPS:$REFLECT

# --- (b) reference build: real scalac. Shared and reused. -------------------
# gitbucket's own `scalacOptions` minus the warning settings and the optimiser,
# the same set `gitbucket_measure.sh` passes us. `-Wconf:cat=scala3-migration:s`
# is added because `-Xsource:3-cross` turns nsc's migration advice into errors
# and gitbucket's build has its own `-Wconf` for that; it changes nothing that
# is emitted.
if [[ ${REUSE_SCALAC:-1} != 1 || ! -d $DIR/out-scalac ]]; then
  echo "== compiling gitbucket with real scalac (once) =="
  rm -rf $WORK/out-scalac; mkdir -p $WORK/out-scalac
  $SCALAC "${FILES[@]}" -d $WORK/out-scalac -cp "$CP_DEPS" \
    -Xsource:3-cross -language:postfixOps -Wconf:cat=scala3-migration:s \
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
# Same inputs and same flags as `tests/gitbucket_measure.sh`.
if [[ ${REUSE_RS:-0} != 1 || ! -d $WORK/out-rs ]]; then
  echo "== compiling gitbucket with scala-rs =="
  BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
  if [[ -z ${SCALA_RS:-} ]]; then
    (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>$WORK/build.log \
      || { cat $WORK/build.log; exit 1; }
  fi
  rm -rf $WORK/out-rs; mkdir -p $WORK/out-rs
  COMPILER_EXIT=0
  "$BIN" compile "${FILES[@]}" -d $WORK/out-rs -cp "$CP_DEPS" \
    -Xsource:3-cross -language:postfixOps --scala-library $LIB \
    > $WORK/rs.log 2>&1 || COMPILER_EXIT=$?
  E=$(grep -c '^error' $WORK/rs.log || true)
  C=$(find $WORK/out-rs -name '*.class' | wc -l | tr -d ' ')
  echo "   scala-rs: files=${#FILES[@]} java=${#JAVA_FILES[@]} errors=$E classes=$C compiler_exit=$COMPILER_EXIT"
  if (( COMPILER_EXIT != 0 || E != 0 || C == 0 || ${#FILES[@]} == 0 )); then
    echo "gitbucket compile failed; see $WORK/rs.log" >&2
    exit 1
  fi
fi
RS_CLASSES=$(find $WORK/out-rs -name '*.class' | wc -l | tr -d ' ')

# Structural check over *every* class, not only the ones the programs reach: a
# branch offset that wrapped, or a method over the 64 KB JVMS 4.7.3 allows.
# Invisible to the loader and to the runs below.
LINT=$(LINT_JOBS=${GBRUN_JOBS:-${LINT_JOBS:-4}} python3 "$ROOT/tests/classfile_lint.py" $WORK/out-rs | tail -1)
echo "   lint: $LINT"

SUPPORT=($(find $ROOT/tests/gbrun/support -name '*.scala' | sort))
CP_A=$WORK/out-rs:$RES:$CP_DEPS:$LIB
CP_B=$DIR/out-scalac:$RES:$CP_DEPS:$LIB
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
if [[ -n ${KNOWN+x} ]]; then
  for p in ${=KNOWN}; do KNOWN_WHY[$p]="held out by KNOWN="; done
else
  KNOWN_WHY=(
    Utils "'Accounts returning ... insert row' infers the inserted id as Nothing (docs/notes/rh-cats-gitbucket-run.md)"
  )
fi

PROGS=($@)
if [[ ${#PROGS[@]} -eq 0 ]]; then
  PROGS=($(cd $ROOT/tests/gbrun && ls *.scala | sed 's/\.scala$//' | sort))
fi

PASS=0; DIFF=0; FAIL=0; PICKLE_FAIL=0
UNEXPECTED_PASS=()
for p in $PROGS; do
  src=$ROOT/tests/gbrun/$p.scala
  out=$WORK/progs/$p
  rm -rf $out; mkdir -p $out/b $out/a
  # --- codegen axis: one binary, two libraries ---
  if ! $SCALAC $src "${SUPPORT[@]}" -d $out/b -cp "$CP_B" "${CLIENT_OPTS[@]}" \
       > $out/compile-b.log 2>&1; then
    echo "COMPILE-FAIL-B $p   (see $out/compile-b.log)"; FAIL=$((FAIL+1)); continue
  fi
  rb=0; java -Xverify:all -cp $out/b:$CP_B Main > $out/b.out 2> $out/b.err || rb=$?
  ra=0; java -Xverify:all -cp $out/b:$CP_A Main > $out/a.out 2> $out/a.err || ra=$?
  if (( rb != 0 )); then
    echo "REF-FAIL     $p   scalac-built gitbucket could not run it (see $out/b.err)"
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
    if ! $SCALAC $src "${SUPPORT[@]}" -d $out/a -cp "$CP_A" "${CLIENT_OPTS[@]}" \
         > $out/compile-a.log 2>&1; then
      echo "PICKLE-FAIL  $p   scalac cannot compile against our gitbucket (see $out/compile-a.log)"
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
