#!/bin/zsh
# Compile slick's main sources with scala-rs and report the error count.
# Usage: tests/slick_measure.sh [extra scala-rs args...]
set -e
SP=/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/slick
SRC=$SP/slick/slick/src/main
COMPAT=$SP/slick/slick-compat-collections/src/main/scala-2.13+
# Default to *this* checkout's binary, not a fixed path: run from a git
# worktree, the hardcoded path measured the parent repo's build and an agent's
# own changes appeared to do nothing.
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
# The release binary is not what `cargo test` builds; measuring a stale one
# silently reports the previous commit's numbers.
if [[ -z ${SCALA_RS:-} ]]; then
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>/tmp/slick_measure_build.log \
    || { cat /tmp/slick_measure_build.log; exit 1; }
fi
# slick keeps seven sources as FreeMarker templates that its own build
# expands. Measuring without them reports errors scalac would report too, so
# expand them here and compile them alongside.
# Every path this script writes is per-invocation: two measurements running at
# once (an agent's copy and this one) shared `generated/`, `measure-out/` and
# `measure.txt`, and reported each other's numbers.
RUN=${SLICK_RUN:-$SP/run-$$}
GEN=${SLICK_GEN:-$RUN/generated}
rm -rf $GEN
python3 "$ROOT/tests/expand_fm.py" $SRC/scala $GEN >/dev/null
FILES=($(find $SRC/scala $SRC/scala-2 $COMPAT $GEN -name '*.scala' | sort))
OUT=${SLICK_OUT:-$RUN/out}
rm -rf $OUT; mkdir -p $OUT
# slick's build.sbt depends on scala-reflect (its macros import
# scala.reflect.macros.blackbox.Context); without the jar even real scalac
# cannot compile ShapedValue.scala, so measuring without it asks for the
# impossible. Appended here rather than in the shared deps.cp so a stale
# scratchpad state cannot lose it.
REFLECT=/tmp/scala-2.13.16/lib/scala-reflect.jar
$BIN compile "${FILES[@]}" -d $OUT -cp "$(cat $SP/deps.cp):$REFLECT" -Xsource:3 \
  --scala-library /tmp/scala-rs-lib/scala-library-2.13.16.jar "$@" > ${SLICK_LOG:-$SP/measure.txt} 2>&1 || true
ERRORS=$(grep -c '^error' ${SLICK_LOG:-$SP/measure.txt} || true)
CLASSES=$(find $OUT -name '*.class' | wc -l | tr -d ' ')
# Cascades inflate the raw count; files-with-errors is the honest progress metric.
BADFILES=$(grep -A 2 '^error' ${SLICK_LOG:-$SP/measure.txt} | grep -oE '(src/main|generated)/[^:]*' | sort -u | wc -l | tr -d ' ')
rm -rf $RUN
echo "files=${#FILES[@]} errors=$ERRORS files_with_errors=$BADFILES classes=$CLASSES"
