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
#   SLICK_RUN_DIR   work dir (default: <scratchpad>/slickrun)
#   SLICK_RUN_ID    name of the private area inside it (default: a hash of
#                   $ROOT, so every worktree gets its own)
#   RUNS=n          execute each program n times per side (default 3).  A
#                   program counts as ok only if all n attempts pass; the
#                   per-program "m/n" is printed, so a retry can never hide a
#                   failure -- it only tells you the failure is intermittent.
#   REUSE_RS=1      do not recompile slick with scala-rs (reuse $WORK/out-rs)
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
set -e
SP=/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/slick
DIR=${SLICK_RUN_DIR:-/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/slickrun}
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
SCALAC=/tmp/scala-2.13.16/bin/scalac
REFLECT=/tmp/scala-2.13.16/lib/scala-reflect.jar
LIB=/tmp/scala-rs-lib/scala-library-2.13.16.jar
CCACHE=$HOME/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2
H2=$CCACHE/com/h2database/h2/2.1.214/h2-2.1.214.jar
RUNS=${RUNS:-3}

if [[ ! -x $SCALAC || ! -s $LIB || ! -d $SP/slick/.git || ! -s $SP/deps.cp ]]; then
  echo "toolchain or slick checkout missing; run tests/slick_measure.sh once first (it self-restores)" >&2
  exit 1
fi
[[ -s $H2 ]] || { echo "H2 jar not in the Coursier cache: $H2" >&2; exit 1; }

SRC=$SP/slick/slick/src/main
COMPAT=$SP/slick/slick-compat-collections/src/main/scala-2.13+
RES=$SRC/resources
DEPS=$(cat $SP/deps.cp):$REFLECT

# --- private work area, one per worktree, locked ----------------------------
ID=${SLICK_RUN_ID:-$(printf '%s' "$ROOT" | shasum | cut -c1-10)}
WORK=$DIR/w-$ID
mkdir -p $WORK
LOCK=$WORK/.lock
if ! mkdir $LOCK 2>/dev/null; then
  OWNER=$(cat $LOCK/pid 2>/dev/null || echo '?')
  if [[ $OWNER == '?' ]] || kill -0 $OWNER 2>/dev/null; then
    echo "another slick_run.sh (pid $OWNER) is using $WORK." >&2
    echo "wait for it, or run with SLICK_RUN_ID=<something else>." >&2
    exit 1
  fi
  echo "note: taking over the lock of dead pid $OWNER" >&2
fi
echo $$ > $LOCK/pid
trap 'rm -rf $LOCK' EXIT INT TERM

GEN=$WORK/generated
rm -rf $GEN
python3 "$ROOT/tests/expand_fm.py" $SRC/scala $GEN >/dev/null
FILES=($(find $SRC/scala $SRC/scala-2 $COMPAT $GEN -name '*.scala' | sort))

# --- (b) reference build: real scalac. Slow; shared and reused. -------------
# It depends only on the slick checkout and on real scalac, so every worktree
# can use the same copy.  Built into a private directory and published with a
# rename, so a concurrent reader never sees a half-written build.
if [[ ${REUSE_SCALAC:-1} != 1 || ! -d $DIR/out-scalac ]]; then
  echo "== compiling slick with real scalac (slow, once) =="
  rm -rf $WORK/out-scalac; mkdir -p $WORK/out-scalac
  $SCALAC "${FILES[@]}" -d $WORK/out-scalac -cp "$DEPS" -Xsource:3-cross \
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
if [[ ${REUSE_RS:-0} != 1 || ! -d $WORK/out-rs ]]; then
  echo "== compiling slick with scala-rs =="
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>$WORK/build.log \
    || { cat $WORK/build.log; exit 1; }
  rm -rf $WORK/out-rs; mkdir -p $WORK/out-rs
  "$ROOT/target/release/scala-rs" compile "${FILES[@]}" -d $WORK/out-rs -cp "$DEPS" \
    -Xsource:3 --scala-library $LIB > $WORK/rs.log 2>&1 || true
  E=$(grep -c '^error' $WORK/rs.log || true)
  C=$(find $WORK/out-rs -name '*.class' | wc -l | tr -d ' ')
  # Print files= alongside errors= and classes=: a truncated slick checkout
  # reads as a clean build otherwise.
  echo "   scala-rs: files=${#FILES[@]} errors=$E classes=$C"
  # Structural check over *every* class we just wrote, not only the ones the
  # programs below happen to call: a branch offset that wrapped, or a method
  # over the 64 KB the format allows. Neither is visible to the loader check
  # in slick_subset.sh (which stops after the constant pool) nor to the runs
  # below (which reach a fraction of the methods). ~3 s for 1600 classes.
  python3 "$ROOT/tests/classfile_lint.py" $WORK/out-rs | tail -20
fi

CP_A=$WORK/out-rs:$RES:$DEPS:$H2:$LIB
CP_B=$DIR/out-scalac:$RES:$DEPS:$H2:$LIB
[[ ${MODE:-b} == a ]] && CP_COMPILE=$CP_A || CP_COMPILE=$CP_B

PROGS=($@)
if [[ ${#PROGS[@]} -eq 0 ]]; then
  PROGS=($(cd $ROOT/tests/slick_progs && ls *.scala | sed 's/\.scala$//' | sort))
fi

load() { uptime | sed 's/.*load averages*: //'; }

mkdir -p $WORK/progs
PASS=0; DIFF=0; FAIL=0; ATT=0; ATT_OK=0
for p in $PROGS; do
  src=$ROOT/tests/slick_progs/$p.scala
  out=$WORK/progs/$p
  rm -rf $out; mkdir -p $out
  if ! $SCALAC $src -d $out -cp "$CP_COMPILE" > $out/compile.log 2>&1; then
    echo "COMPILE-FAIL $p   (see $out/compile.log)"; FAIL=$((FAIL+1)); continue
  fi
  # Execute RUNS times.  Every attempt that is not a clean byte-identical pass
  # is reported and its stdout/stderr kept under a per-attempt name, so an
  # intermittent failure is visible instead of being averaged away.
  okc=0; verdict=ok; firstbad=0
  for k in $(seq 1 $RUNS); do
    ATT=$((ATT+1))
    ra=0; java -cp $out:$CP_A Main > $out/a.out 2> $out/a.err || ra=$?
    rb=0; java -cp $out:$CP_B Main > $out/b.out 2> $out/b.err || rb=$?
    if [[ $ra != 0 || $rb != 0 ]]; then
      verdict=fail
      echo "   attempt $k/$RUNS $p: rs=$ra scalac=$rb  load=$(load)"
    elif ! cmp -s $out/a.out $out/b.out; then
      [[ $verdict == fail ]] || verdict=diff
      echo "   attempt $k/$RUNS $p: stdout differs  load=$(load)"
    else
      okc=$((okc+1)); ATT_OK=$((ATT_OK+1)); continue
    fi
    [[ $firstbad != 0 ]] || firstbad=$k
    for f in a.out a.err b.out b.err; do cp $out/$f $out/attempt$k-$f 2>/dev/null || true; done
  done
  case $verdict in
    ok)   echo "ok           $p   $okc/$RUNS"; PASS=$((PASS+1));;
    diff) echo "DIFF         $p   $okc/$RUNS  (diff $out/attempt$firstbad-b.out $out/attempt$firstbad-a.out)"; DIFF=$((DIFF+1));;
    fail) echo "RUN-FAIL     $p   $okc/$RUNS  (see $out/attempt*-a.err $out/attempt*-b.err)"; FAIL=$((FAIL+1));;
  esac
done
echo "progs=${#PROGS[@]} ok=$PASS diff=$DIFF fail=$FAIL  runs=$RUNS attempts=$ATT_OK/$ATT  (compile-cp=${MODE:-b}, work=$WORK)"
[[ $DIFF -eq 0 && $FAIL -eq 0 ]] || exit 1
