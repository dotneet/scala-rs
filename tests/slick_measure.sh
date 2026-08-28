#!/bin/zsh
# Compile slick's main sources with scala-rs and report the error count.
# Usage: tests/slick_measure.sh [extra scala-rs args...]
set -e
SP=/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/slick
SRC=$SP/slick/slick/src/main
COMPAT=$SP/slick/slick-compat-collections/src/main/scala-2.13+
BIN=${SCALA_RS:-/Users/shinji/projects/scala-rs/target/release/scala-rs}
FILES=($(find $SRC/scala $SRC/scala-2 $COMPAT -name '*.scala' | sort))
OUT=${SLICK_OUT:-$SP/measure-out}
rm -rf $OUT; mkdir -p $OUT
$BIN compile "${FILES[@]}" -d $OUT -cp "$(cat $SP/deps.cp)" -Xsource:3 \
  --scala-library /tmp/scala-rs-lib/scala-library-2.13.16.jar "$@" > $SP/measure.txt 2>&1 || true
ERRORS=$(grep -c '^error' $SP/measure.txt || true)
CLASSES=$(find $OUT -name '*.class' | wc -l | tr -d ' ')
# Cascades inflate the raw count; files-with-errors is the honest progress metric.
BADFILES=$(grep -A 2 '^error' $SP/measure.txt | grep -o 'src/main/[^:]*' | sort -u | wc -l | tr -d ' ')
echo "files=${#FILES[@]} errors=$ERRORS files_with_errors=$BADFILES classes=$CLASSES"
