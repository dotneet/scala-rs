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
#   REUSE_RS=1      do not recompile slick with scala-rs (reuse $DIR/out-rs)
#   REUSE_SCALAC=0  force recompiling slick with real scalac (slow, ~4 min)
#   MODE=b          which slick sits on the *compile* classpath of the client
#                   programs: "b" (default) = the scalac-built slick, so the
#                   client binary is beyond suspicion; "a" = the scala-rs-built
#                   slick, which additionally makes real scalac read scala-rs's
#                   ScalaSignature pickles and run scala-rs's macro classfiles.
set -e
SP=/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/slick
DIR=${SLICK_RUN_DIR:-/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/slickrun}
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
SCALAC=/tmp/scala-2.13.16/bin/scalac
REFLECT=/tmp/scala-2.13.16/lib/scala-reflect.jar
LIB=/tmp/scala-rs-lib/scala-library-2.13.16.jar
CCACHE=$HOME/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2
H2=$CCACHE/com/h2database/h2/2.1.214/h2-2.1.214.jar

if [[ ! -x $SCALAC || ! -s $LIB || ! -d $SP/slick/.git || ! -s $SP/deps.cp ]]; then
  echo "toolchain or slick checkout missing; run tests/slick_measure.sh once first (it self-restores)" >&2
  exit 1
fi
[[ -s $H2 ]] || { echo "H2 jar not in the Coursier cache: $H2" >&2; exit 1; }

SRC=$SP/slick/slick/src/main
COMPAT=$SP/slick/slick-compat-collections/src/main/scala-2.13+
RES=$SRC/resources
DEPS=$(cat $SP/deps.cp):$REFLECT

mkdir -p $DIR
GEN=$DIR/generated
rm -rf $GEN
python3 "$ROOT/tests/expand_fm.py" $SRC/scala $GEN >/dev/null
FILES=($(find $SRC/scala $SRC/scala-2 $COMPAT $GEN -name '*.scala' | sort))

# --- (b) reference build: real scalac. Slow; kept and reused. ---------------
if [[ ${REUSE_SCALAC:-1} != 1 || ! -d $DIR/out-scalac ]]; then
  echo "== compiling slick with real scalac (slow, once) =="
  rm -rf $DIR/out-scalac; mkdir -p $DIR/out-scalac
  $SCALAC "${FILES[@]}" -d $DIR/out-scalac -cp "$DEPS" -Xsource:3-cross \
    > $DIR/scalac.log 2>&1 || { echo "real scalac failed; see $DIR/scalac.log" >&2; exit 1; }
fi

# --- (a) build under test: scala-rs -----------------------------------------
if [[ ${REUSE_RS:-0} != 1 || ! -d $DIR/out-rs ]]; then
  echo "== compiling slick with scala-rs =="
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>$DIR/build.log \
    || { cat $DIR/build.log; exit 1; }
  rm -rf $DIR/out-rs; mkdir -p $DIR/out-rs
  "$ROOT/target/release/scala-rs" compile "${FILES[@]}" -d $DIR/out-rs -cp "$DEPS" \
    -Xsource:3 --scala-library $LIB > $DIR/rs.log 2>&1 || true
  E=$(grep -c '^error' $DIR/rs.log || true)
  C=$(find $DIR/out-rs -name '*.class' | wc -l | tr -d ' ')
  echo "   scala-rs: errors=$E classes=$C"
fi

CP_A=$DIR/out-rs:$RES:$DEPS:$H2:$LIB
CP_B=$DIR/out-scalac:$RES:$DEPS:$H2:$LIB
[[ ${MODE:-b} == a ]] && CP_COMPILE=$CP_A || CP_COMPILE=$CP_B

PROGS=($@)
if [[ ${#PROGS[@]} -eq 0 ]]; then
  PROGS=($(cd $ROOT/tests/slick_progs && ls *.scala | sed 's/\.scala$//' | sort))
fi

mkdir -p $DIR/progs
PASS=0; DIFF=0; FAIL=0
for p in $PROGS; do
  src=$ROOT/tests/slick_progs/$p.scala
  out=$DIR/progs/$p
  rm -rf $out; mkdir -p $out
  if ! $SCALAC $src -d $out -cp "$CP_COMPILE" > $out/compile.log 2>&1; then
    echo "COMPILE-FAIL $p   (see $out/compile.log)"; FAIL=$((FAIL+1)); continue
  fi
  ra=0; java -cp $out:$CP_A Main > $out/a.out 2> $out/a.err || ra=$?
  rb=0; java -cp $out:$CP_B Main > $out/b.out 2> $out/b.err || rb=$?
  if [[ $ra != 0 || $rb != 0 ]]; then
    echo "RUN-FAIL     $p   rs=$ra scalac=$rb   (see $out/a.err $out/b.err)"
    FAIL=$((FAIL+1)); continue
  fi
  if cmp -s $out/a.out $out/b.out; then
    echo "ok           $p"; PASS=$((PASS+1))
  else
    echo "DIFF         $p   (diff $out/b.out $out/a.out)"; DIFF=$((DIFF+1))
  fi
done
echo "progs=${#PROGS[@]} ok=$PASS diff=$DIFF fail=$FAIL  (compile-cp=${MODE:-b})"
