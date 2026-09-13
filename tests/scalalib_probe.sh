#!/bin/zsh
# Compile the whole scala/scala `src/library` (the arrangement
# `tests/scalalib_measure.sh` measures) from a WRITABLE COPY of its sources, so a
# probe can be written *into* a library file.
#
# Why this exists. Several standard-library roots are invisible to any reduction
# written outside the library: they need the run's own sources to supply the
# class whose type parameters are involved, and in jar mode the prelude supplies
# it instead. Two slices in a row reduced such a root only by being able to
# insert
#
#     val dbg: Nothing = <the expression in question>
#
# into the library method and read the "found:" half of the mismatch -- which is
# how `IterableOps.groupBy`'s `HashMap[K, AnyRef]` turned out to be an
# as-seen-from capture in `m.iterator` and not lower-bound inference at all
# (`docs/scala-library.md`, `agent/libfinal`). Each rebuilt the harness from
# scratch; this is it, kept.
#
# Usage:
#   tests/scalalib_probe.sh init            # make/refresh the writable copy
#   tests/scalalib_probe.sh [tag]           # compile it, print errors
#   tests/scalalib_probe.sh add p.scala     # compile it plus one extra file,
#                                           # printing only that file's errors
#
# `SCALALIB_PROBE_DIR` is the writable copy (default under the scratchpad's
# sibling `/private/tmp/scala-rs-libprobe`); `SCALA_RS` the binary to use.
# Nothing here is run by a gate: it is a debugging tool.
set -e
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
WORK=${SCALALIB_PROBE_DIR:-/private/tmp/scala-rs-libprobe}
# The pristine sources and the 33 Java classfiles, both laid down by
# `tests/scalalib_measure.sh`; run that once first if they are missing.
SP=${SCALALIB_SRC_DIR:-/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/scalalib}
SRC=$SP/scala/src/library
JAVACP=$SP/javacp/keep
MODE=${1:-run}

if [[ $MODE == init || ! -d $WORK/src ]]; then
  if [[ ! -d $SRC || ! -d $JAVACP ]]; then
    echo "missing $SRC or $JAVACP -- run tests/scalalib_measure.sh once first" >&2
    exit 1
  fi
  rm -rf $WORK/src; mkdir -p $WORK/src
  cp -R $SRC/. $WORK/src/
  echo "copied $(find $WORK/src -name '*.scala' | wc -l | tr -d ' ') sources to $WORK/src"
  [[ $MODE == init ]] && exit 0
fi

FILES=($(find $WORK/src -name '*.scala' | sort))
if [[ $MODE == add ]]; then
  PROBE=$2
  [[ -f $PROBE ]] || { echo "usage: $0 add <probe.scala>" >&2; exit 1 }
  TAG=$(basename $PROBE .scala)
  FILES+=($PROBE)
else
  PROBE=""
  TAG=${1:-run}
fi
OUT=$WORK/out-$TAG
rm -rf $OUT; mkdir -p $OUT
LOG=$OUT/log.txt
# Same flags as `tests/scalalib_measure.sh`: `-no-specialization` (nsc's own) and
# `--no-scala-library` with only the library's *Java* half on the classpath.
$BIN compile "${FILES[@]}" -d $OUT -cp $JAVACP \
  -no-specialization --no-scala-library > $LOG 2>&1 || true
ERRORS=$(grep -c '^error' $LOG || true)
CLASSES=$(find $OUT -name '*.class' | wc -l | tr -d ' ')
if [[ -n $PROBE ]]; then
  echo "--- $TAG.scala diagnostics ---"
  grep -A 5 '^error' $LOG | grep -B 1 -A 4 "$TAG.scala" | head -60
else
  echo "--- diagnostics ---"
  grep -A 5 '^error' $LOG | head -80
fi
echo "files=${#FILES[@]} errors=$ERRORS classes=$CLASSES log=$LOG"
