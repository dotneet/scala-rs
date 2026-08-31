#!/bin/zsh
# Compile slick's main sources with scala-rs and report the error count.
# Usage: tests/slick_measure.sh [extra scala-rs args...]
set -e
SP=/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/slick
# --- self-restore -----------------------------------------------------------
# Everything under /tmp vanishes on reboot (it did, 2026-08-31). Rebuild the
# toolchain from the Coursier cache and re-clone slick at the pinned revision
# whenever a piece is missing, so a reboot costs one slow run instead of a
# debugging session.
SLICK_REV=475fc6e7719867025e832fa0e6ac7fb21b36bbc3
CCACHE=$HOME/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2
if [[ ! -x /tmp/scala-2.13.16/bin/scalac ]]; then
  mkdir -p /tmp/scala-2.13.16/bin /tmp/scala-2.13.16/lib /tmp/scala-rs-lib
  cp $CCACHE/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar /tmp/scala-rs-lib/
  cp $CCACHE/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar /tmp/scala-2.13.16/lib/scala-library.jar
  cp $CCACHE/org/scala-lang/scala-reflect/2.13.16/scala-reflect-2.13.16.jar /tmp/scala-2.13.16/lib/scala-reflect.jar
  cp $CCACHE/org/scala-lang/scala-compiler/2.13.16/scala-compiler-2.13.16.jar /tmp/scala-2.13.16/lib/scala-compiler.jar
  printf '#!/bin/sh\nL=/tmp/scala-2.13.16/lib\nexec java -cp "$L/scala-compiler.jar:$L/scala-library.jar:$L/scala-reflect.jar" scala.tools.nsc.Main "$@"\n' > /tmp/scala-2.13.16/bin/scalac
  chmod +x /tmp/scala-2.13.16/bin/scalac
fi
if [[ ! -d $SP/slick/.git ]]; then
  mkdir -p $SP; rm -rf $SP/slick
  git clone https://github.com/slick/slick.git $SP/slick >/dev/null 2>&1
  (cd $SP/slick && git checkout -q $SLICK_REV)
fi
if [[ ! -s $SP/deps.cp ]]; then
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
  done | paste -sd: - > $SP/deps.cp
fi
# ---------------------------------------------------------------------------
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
