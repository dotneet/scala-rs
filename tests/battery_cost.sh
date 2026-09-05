#!/usr/bin/env bash
# Time every check in the verification battery and print what each one costs.
#
# The point is not the total. The point is to notice when one line moves for a
# reason that has nothing to do with the work: on 2026-09-05 `cats_measure.sh`
# was taking 334 s to report 2929 errors, where the same 339 files took 4 s once
# `-Ykind-projector` was passed. Nobody was looking, so every slice paid it.
#
# Run this every few slices, and whenever a check feels slower than it was.
# Compare against the table in `.agent-brief.md`; a line that has doubled is a
# bug report, not a fact of life.
#
#   tests/battery_cost.sh              # every check
#   tests/battery_cost.sh slick cats   # only those
#
# Writes its logs under a private directory so it never clobbers a baseline.
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=${BATTERY_OUT:-/tmp/scala-rs-battery-$$}
mkdir -p "$OUT"
cd "$ROOT"

# Build once up front: otherwise the first check pays for the whole compile and
# looks like it regressed.
cargo build -p scala-rs-cli --release >/dev/null 2>&1 || {
  echo "build failed; nothing to time" >&2
  exit 1
}

want() {
  [[ $# -eq 0 ]] && return 0
  local n=$1; shift
  for a in "$@"; do [[ $n == "$a" ]] && return 0; done
  return 1
}

run() {
  local name=$1 cmd=$2
  want "$name" "${SELECT[@]}" || return 0
  local s e
  s=$(date +%s)
  eval "$cmd" >"$OUT/$name.log" 2>&1
  e=$(date +%s)
  printf '%-22s %5d s\n' "$name" "$((e - s))"
}

SELECT=("$@")

echo "=== battery cost (seconds) ==="
run slick      "SLICK_LOG=$OUT/slick.txt ./tests/slick_measure.sh"
run cats       "CATS_LOG=$OUT/cats.txt ./tests/cats_measure.sh"
run gitbucket  "GITBUCKET_LOG=$OUT/gb.txt ./tests/gitbucket_measure.sh"
run scalalib   "SCALALIB_LOG=$OUT/lib.txt ./tests/scalalib_measure.sh"
run corpus     "CORPUS_LOG=$OUT/corpus.tsv ./tests/scala_corpus.sh"
run spec       "SPEC_LOG=$OUT/spec.tsv ./tests/spec_classfiles.sh"
run slick_run  "./tests/slick_run.sh"
run subset     "SLICK_SEED_LOG=$OUT/slick.txt ./tests/slick_subset.sh"
run workspace  "cargo test --workspace --release --no-fail-fast"

echo
echo "logs in $OUT"
