#!/bin/zsh
# Time scala-rs on slick's 184 sources and report wall + CPU time.
#
# Unlike tests/slick_measure.sh (which reports *correctness*: errors, classes),
# this reports *speed*. It stops after the phase you ask for so a typer-only
# change can be measured without paying for codegen:
#
#   tests/bench.sh            # default: --typer (99% of the time lives here)
#   tests/bench.sh --parse    # parse only
#   tests/bench.sh --full     # parse + typer + codegen
#   REPS=3 tests/bench.sh     # repeat and report every run
#
# The machine usually has other agents on it, so wall time drifts. `user` (CPU
# time) is the number to compare across commits; wall time is reported too so a
# run taken on a loaded machine is visible as such.
set -e
SP=/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad
BENCH=${BENCH_DIR:-$SP/bench}
SLICKSP=$SP/slick
SLICK_REV=475fc6e7719867025e832fa0e6ac7fb21b36bbc3
CCACHE=$HOME/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}

# --- self-restore (same contract as slick_measure.sh) -----------------------
if [[ ! -f /tmp/scala-rs-lib/scala-library-2.13.16.jar ]]; then
  mkdir -p /tmp/scala-rs-lib /tmp/scala-2.13.16/lib
  cp $CCACHE/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar /tmp/scala-rs-lib/
  cp $CCACHE/org/scala-lang/scala-reflect/2.13.16/scala-reflect-2.13.16.jar /tmp/scala-2.13.16/lib/scala-reflect.jar
fi
if [[ ! -d $SLICKSP/slick/.git ]]; then
  mkdir -p $SLICKSP; rm -rf $SLICKSP/slick
  git clone https://github.com/slick/slick.git $SLICKSP/slick >/dev/null 2>&1
  (cd $SLICKSP/slick && git checkout -q $SLICK_REV)
fi
if [[ ! -s $SLICKSP/deps.cp ]]; then
  for j in com/typesafe/config/1.4.9/config-1.4.9.jar \
           org/reactivestreams/reactive-streams/1.0.4/reactive-streams-1.0.4.jar \
           org/slf4j/slf4j-api/2.0.18/slf4j-api-2.0.18.jar \
           org/typelevel/cats-core_2.13/2.13.0/cats-core_2.13-2.13.0.jar \
           org/typelevel/cats-kernel_2.13/2.13.0/cats-kernel_2.13-2.13.0.jar \
           org/typelevel/cats-effect_2.13/3.7.1/cats-effect_2.13-3.7.1.jar \
           org/typelevel/cats-effect-kernel_2.13/3.7.1/cats-effect-kernel_2.13-3.7.1.jar \
           org/typelevel/cats-effect-std_2.13/3.7.1/cats-effect-std_2.13-3.7.1.jar \
           org/typelevel/cats-mtl_2.13/1.6.0/cats-mtl_2.13-1.6.0.jar \
           org/scodec/scodec-bits_2.13/1.2.4/scodec-bits_2.13-1.2.4.jar \
           co/fs2/fs2-core_2.13/3.13.0/fs2-core_2.13-3.13.0.jar; do
    echo $CCACHE/$j
  done | paste -sd: - > $SLICKSP/deps.cp
fi

# --- the file list ----------------------------------------------------------
# Pinned so every commit is timed on the same 184 files. Regenerated only if
# missing (the FreeMarker templates must be expanded, as in slick_measure.sh).
SRC=$SLICKSP/slick/slick/src/main
COMPAT=$SLICKSP/slick/slick-compat-collections/src/main/scala-2.13+
if [[ ! -s $BENCH/files.txt ]]; then
  mkdir -p $BENCH
  rm -rf $BENCH/gen
  python3 "$ROOT/tests/expand_fm.py" $SRC/scala $BENCH/gen >/dev/null
  find $BENCH/gen $SRC/scala $SRC/scala-2 $COMPAT -name '*.scala' | sort > $BENCH/files.txt
fi
FILES=("${(@f)$(cat $BENCH/files.txt)}")

if [[ -z ${SCALA_RS:-} ]]; then
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>/tmp/bench_build.log \
    || { cat /tmp/bench_build.log; exit 1; }
fi
BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}

PHASE=${1:---typer}
[[ $PHASE == --full ]] && PHASE=""
REFLECT=/tmp/scala-2.13.16/lib/scala-reflect.jar
RUN=${BENCH_RUN:-$BENCH/run-$$}
mkdir -p $RUN
trap 'rm -rf $RUN' EXIT

echo "bin=$BIN files=${#FILES[@]} phase=${PHASE:---full} load=$(uptime | sed 's/.*averages: //')"
for i in $(seq 1 ${REPS:-2}); do
  /usr/bin/time -p $BIN compile "${FILES[@]}" -d $RUN/out \
    -cp "$(cat $SLICKSP/deps.cp):$REFLECT" -Xsource:3 \
    --scala-library /tmp/scala-rs-lib/scala-library-2.13.16.jar ${PHASE:+$PHASE} \
    > $RUN/log 2>$RUN/time || true
  REAL=$(awk '/^real/{print $2}' $RUN/time)
  USER=$(awk '/^user/{print $2}' $RUN/time)
  SYS=$(awk '/^sys/{print $2}' $RUN/time)
  ERRS=$(grep -c '^error' $RUN/log || true)
  echo "run$i real=${REAL}s user=${USER}s sys=${SYS}s errors=$ERRS"
done
