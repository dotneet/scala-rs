#!/bin/zsh
# Compile slick's *test kit* with scala-rs and report the error count.
#
# `slick_measure.sh` measures slick's own 184 main sources. The testkit is the
# next layer out: it is a *user* of slick's API, so it exercises the inferred
# types that slick's classfiles export rather than the ones the typer just
# computed from source. The two stages are:
#
#   main  slick-testkit/src/main + the four modules testkit's compile scope
#         depends on (codegen, hikaricp, slick-future, slick-zio).
#   test  slick-testkit/src/test, on top of stage `main`'s output.
#
# Usage:  tests/testkit_measure.sh [main|test|all] [extra scala-rs args...]
#
# Env:
#   TESTKIT_DIR      scratch root (default .../scratchpad/testkit)
#   SLICK_CLASSES    dir with slick's 184 main sources already compiled.
#                    Compiled here when unset -- that costs 4.5 minutes, so
#                    pass the directory `slick_measure.sh` left behind
#                    (SLICK_OUT=<dir> tests/slick_measure.sh) when iterating.
#   TESTKIT_SLICK_SRC=1  put slick's *sources* in the same compilation instead
#                    of its classfiles. Same program, different supply path:
#                    a diagnostic that appears only with classfiles is a
#                    classfile-emit/read bug, not a testkit bug.
#   TESTKIT_LOG      log path (default $TESTKIT_DIR/<stage>.txt)
set -e
SP=/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/slick
TK=${TESTKIT_DIR:-/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/testkit}
STAGE=${1:-main}
if [[ $STAGE == main || $STAGE == test || $STAGE == all ]]; then shift; else STAGE=main; fi
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
mkdir -p $TK
# slick_measure.sh owns the checkout/toolchain self-restore; reuse it.
if [[ ! -d $SP/slick/.git || ! -s $SP/deps.cp || ! -x /tmp/scala-2.13.16/bin/scalac ]]; then
  echo "slick checkout/toolchain missing -- run tests/slick_measure.sh once first" >&2
  exit 1
fi
# testkit's compile scope adds junit (macro-free, but `org.junit.Test` is an
# annotation every test class carries), HikariCP and ZIO. Not in the shared
# deps.cp because slick's own 184 sources do not need them.
if [[ ! -s $TK/testkit_extra.cp ]]; then
  cs fetch --classpath junit:junit-dep:4.11 com.github.sbt:junit-interface:0.13.3 \
     com.zaxxer:HikariCP:7.1.0 dev.zio:zio_2.13:2.1.26 dev.zio:zio-streams_2.13:2.1.26 \
     dev.zio:zio-interop-cats_2.13:23.1.0.13 > $TK/testkit_extra.cp
fi
if [[ ! -s $TK/testkit_test.cp ]]; then
  cs fetch --classpath com.h2database:h2:2.4.240 ch.qos.logback:logback-classic:1.6.3 \
     org.typelevel:munit-cats-effect_2.13:2.2.0 org.reactivestreams:reactive-streams-tck:1.0.4 \
     org.scalatestplus:testng-7-5_2.13:3.2.17.0 dev.zio:zio-test_2.13:2.1.26 \
     dev.zio:zio-test-sbt_2.13:2.1.26 > $TK/testkit_test.cp
fi
if [[ -z ${SCALA_RS:-} ]]; then
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>$TK/build.log \
    || { cat $TK/build.log; exit 1; }
fi
LIB=/tmp/scala-rs-lib/scala-library-2.13.16.jar
REFLECT=/tmp/scala-2.13.16/lib/scala-reflect.jar
# Coursier resolves a *newer* scala-library (2.13.18) and an older scala-reflect
# into both fetched classpaths. Either on -cp shadows the jar we link against
# and the run measures a different library. Drop them.
strip_lang() { tr ':' '\n' | grep -v '/scala-library-' | grep -v '/scala-reflect-' | paste -sd: - }
EXTRA=$(strip_lang < $TK/testkit_extra.cp)
EXTRATEST=$(strip_lang < $TK/testkit_test.cp)

TKSRC=$SP/slick/slick-testkit/src
# --- slick's own classes ----------------------------------------------------
CLASSES=${SLICK_CLASSES:-}
if [[ -n ${TESTKIT_SLICK_SRC:-} ]]; then
  CLASSES=
elif [[ -z $CLASSES ]]; then
  CLASSES=$TK/slick-out
  if [[ ! -d $CLASSES ]]; then
    SLICK_LOG=$TK/slick-measure.txt SLICK_RUN=$TK/slick-run SLICK_OUT=$CLASSES \
      "$ROOT/tests/slick_measure.sh" >&2
  fi
fi
SLICKSRC=()
if [[ -n ${TESTKIT_SLICK_SRC:-} ]]; then
  GEN=$TK/generated-$$
  rm -rf $GEN
  python3 "$ROOT/tests/expand_fm.py" $SP/slick/slick/src/main/scala $GEN >/dev/null
  SLICKSRC=($(find $SP/slick/slick/src/main/scala $SP/slick/slick/src/main/scala-2 \
              $SP/slick/slick-compat-collections/src/main/scala-2.13+ $GEN -name '*.scala' | sort))
fi

# --- stage main -------------------------------------------------------------
MAINOUT=$TK/main-out
run_stage() {
  local name=$1; shift
  local out=$1; shift
  local cp=$1; shift
  local log=${TESTKIT_LOG:-$TK/$name.txt}
  rm -rf $out; mkdir -p $out
  $BIN compile "$@" -d $out -cp "$cp" -Xsource:3 --scala-library $LIB "${EXTRA_ARGS[@]}" > $log 2>&1 || true
  local errors=$(grep -c '^error' $log || true)
  local classes=$(find $out -name '*.class' | wc -l | tr -d ' ')
  local badfiles=$(grep -A 2 '^error' $log | grep -oE 'slick-[a-z]*/src/[^:]*|/generated-[0-9]*/[^:]*' | sort -u | wc -l | tr -d ' ')
  echo "stage=$name files=$# errors=$errors files_with_errors=$badfiles classes=$classes log=$log"
}
EXTRA_ARGS=("$@")

MAINFILES=($(find $TKSRC/main/scala $TKSRC/main/scala-2 \
             $SP/slick/slick-codegen/src/main/scala \
             $SP/slick/slick-hikaricp/src/main/scala \
             $SP/slick/slick-future/src/main/scala \
             $SP/slick/slick-zio/src/main/scala -name '*.scala' | sort))
if [[ $STAGE == main || $STAGE == all ]]; then
  run_stage main $MAINOUT "${CLASSES:+$CLASSES:}$(cat $SP/deps.cp):$REFLECT:$EXTRA" \
    "${SLICKSRC[@]}" "${MAINFILES[@]}"
fi
# --- stage test -------------------------------------------------------------
# GeneratedCodeTest needs sources sbt generates by running the code generator
# against a live H2; skipped rather than counted as 30 phantom errors.
if [[ $STAGE == test || $STAGE == all ]]; then
  TESTFILES=($(find $TKSRC/test/scala -name '*.scala' \
               ! -name 'GeneratedCodeTest.scala' | sort))
  run_stage test $TK/test-out \
    "$MAINOUT:${CLASSES:+$CLASSES:}$(cat $SP/deps.cp):$REFLECT:$EXTRA:$EXTRATEST" \
    "${TESTFILES[@]}"
fi
