#!/bin/zsh
# The coordinator's merge gate, as one command with one verdict.
#
# Why this exists: on 2026-09-08 the battery was launched as a detached
# background job with no waiter, every step wrote to a different log, and the
# only signal that two tests had failed was an `EXIT=101` nobody looked at.
# The result sat red for five hours. Running the steps by hand is what made
# that possible -- there were eight logs to remember to read, and forgetting
# one looked exactly like success.
#
# So: one script, one log directory, one `VERDICT=` line, one `DONE` sentinel.
# Launch it detached and wait for the sentinel:
#
#   nohup tests/verify_merge.sh > $MY/gate.log 2>&1 &
#   until grep -q '^DONE' $MY/gate.log; do sleep 60; done; grep -E '^(VERDICT|  )' $MY/gate.log
#
# Env:
#   GATE_DIR      where every log goes (default: a fresh per-invocation dir)
#   GATE_LEDGER   corpus baseline to compare against (default: newest tests/baselines/corpus-*.tsv)
#   GATE_SKIP     space-separated steps to skip, e.g. "corpus" -- each skip is
#                 printed in the summary, because a skipped check must never
#                 read as a passing one.
set -u
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
cd "$ROOT"
GATE_DIR=${GATE_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/scala-rs-gate-XXXXXX")}
mkdir -p "$GATE_DIR"
# The ledger to compare the corpus against. `tests/BASELINE.md` names the
# current one and the coordinator keeps it up to date, so read it from there.
#
# It used to be `ls -t … | head -1`. In a git worktree every baseline file
# carries the checkout time, so the "newest" one is arbitrary: a slice's gate
# picked a 106 KB ledger from eleven gates ago instead of the 732 KB current
# one. The gate still said PASS -- it was comparing against nothing. A check
# that reports green over nothing is the exact failure this script exists to
# prevent, so an unresolvable ledger is now a FAIL, not a shrug.
if [[ -n ${GATE_LEDGER:-} ]]; then
  LEDGER=$GATE_LEDGER
else
  LEDGER=$(grep -oE 'tests/baselines/corpus-[0-9a-f]{8}\.tsv|baselines/corpus-[0-9a-f]{8}\.tsv' tests/BASELINE.md 2>/dev/null \
             | sed 's|^baselines/|tests/baselines/|' | head -1)
fi
SKIP=${GATE_SKIP:-}
skipped() { [[ " $SKIP " == *" $1 "* ]] }

HEAD=$(git rev-parse --short=8 HEAD)
DIRTY=$(git status --porcelain | wc -l | tr -d ' ')
print "gate: HEAD=$HEAD dirty_files=$DIRTY dir=$GATE_DIR ledger=${LEDGER:-none}"
if [[ -n ${LEDGER:-} && ! -s $LEDGER ]]; then
  print "gate: ledger $LEDGER does not exist or is empty"
  LEDGER=""
fi

FAIL=()
NOTE=()

step() { print "\n=== $1"; }

step "build"
if ! cargo build --release -p scala-rs-cli > "$GATE_DIR/build.log" 2>&1; then
  print "BUILD FAILED"; tail -30 "$GATE_DIR/build.log"
  print "VERDICT=FAIL (build)"; print "DONE"; exit 1
fi

# --- compile measures -------------------------------------------------------
# Each script validates its own result; we keep the summary line and check the
# few invariants that are not allowed to move.
step "compile measures"
SLICK=$(SLICK_LOG=$GATE_DIR/slick.txt tests/slick_measure.sh 2>&1 | tail -1); print "  slick     $SLICK"
CATS=$(CATS_LOG=$GATE_DIR/cats.txt tests/cats_measure.sh 2>&1 | tail -1);      print "  cats      $CATS"
GB=$(GITBUCKET_LOG=$GATE_DIR/gitbucket.txt tests/gitbucket_measure.sh 2>&1 | tail -1); print "  gitbucket $GB"
LIB=$(SCALALIB_LOG=$GATE_DIR/scalalib.txt tests/scalalib_measure.sh 2>&1 | tail -1);   print "  library   $LIB"

# A measure that compiled the wrong number of files is not a measure. Only
# slick used to be checked here, so on 2026-09-09 a gate printed
# `VERDICT=PASS` with cats reporting `measurement invalid: no source files`
# (its checkout had been gutted) and gitbucket reporting `files=25` of 353.
# Both sat inside a passing run. That is the same failure as a ledger chosen
# by mtime: a check reporting green over nothing. The file counts are pinned
# here, and `measurement invalid` from any of the four is a FAIL.
check_measure() {  # name, summary line, expected `files=` count
  local what=$1 line=$2 want=$3
  if [[ $line == *"measurement invalid"* ]]; then
    FAIL+=("$what measure invalid: $line"); return
  fi
  local got=$(print -r -- "$line" | grep -oE '(^| )files=[0-9]+' | head -1 | grep -oE '[0-9]+')
  if [[ -z $got ]]; then
    FAIL+=("$what measure printed no files= count: $line")
  elif [[ $got != $want ]]; then
    FAIL+=("$what compiled $got files, expected $want: $line")
  fi
}
check_measure slick     "$SLICK" 184
check_measure cats      "$CATS"  340
check_measure gitbucket "$GB"    354
[[ $GB == *"java_sources=3"* ]] || FAIL+=("gitbucket Java inputs missing: $GB")
# gitbucket compiles clean since `agent/gbzero` and `agent/gbzero2`, and it
# now reaches codegen (1317 classes). Zero is an invariant here too.
[[ $GB == *"errors=0 files_with_errors=0"* ]] || FAIL+=("gitbucket measure: $GB")
check_measure library   "$LIB"   538
[[ $SLICK == *"errors=0 files_with_errors=0 classes=1504"* ]] || FAIL+=("slick measure: $SLICK")
# cats compiles clean since `agent/catszero` (quasiquote patterns made the
# last held-out file compile, so there is no holdout any more). Zero is now
# an invariant: a regression here is a gate failure, not a number to report.
[[ $CATS == *"errors=0 files_with_errors=0"* ]] || FAIL+=("cats measure: $CATS")

# --- execution --------------------------------------------------------------
step "slick execution"
RUN=$(MODE=b tests/slick_run.sh 2>&1 | tail -1); print "  $RUN"
[[ $RUN == *"ok=12 diff=0 fail=0"* ]] || FAIL+=("slick_run: $RUN")

if skipped subset; then NOTE+=("slick_subset SKIPPED"); else
  step "slick subset + class validation"
  SUB=$(SLICK_SEED_LOG=$GATE_DIR/slick.txt tests/slick_subset.sh 2>&1 | tail -3 | tr '\n' ' '); print "  $SUB"
  [[ $SUB == *"verified=1504 failed=0"* && $SUB == *"lint_problems=0"* ]] || FAIL+=("slick_subset: $SUB")
fi

# --- workspace suite --------------------------------------------------------
if skipped tests; then NOTE+=("workspace tests SKIPPED"); else
  step "cargo test --workspace --release"
  cargo test --workspace --release --no-fail-fast > "$GATE_DIR/tests.log" 2>&1
  T=$(grep '^test result:' "$GATE_DIR/tests.log" | awk -F'[ ;]' '{p+=$4; f+=$7} END {print NR" rows, "p" passed, "f" failed"}')
  print "  $T"
  NFAIL=$(grep '^test result:' "$GATE_DIR/tests.log" | awk -F'[ ;]' '{f+=$7} END {print f+0}')
  if [[ $NFAIL -ne 0 ]]; then
    FAIL+=("workspace tests: $T")
    print "  failing:"; grep -A8 '^failures:' "$GATE_DIR/tests.log" | grep -E '^    [a-z_0-9]+' | sort -u | sed 's/^/   /'
  fi
fi

# --- corpus -----------------------------------------------------------------
if skipped corpus; then NOTE+=("corpus SKIPPED"); else
  step "scala/scala corpus (full)"
  CORPUS_SIZE=full CORPUS_LOG=$GATE_DIR/corpus.tsv tests/scala_corpus.sh > "$GATE_DIR/corpus.log" 2>&1
  grep -E '^(pos|neg|run): total' "$GATE_DIR/corpus.log" | sed 's/^/  /'
  if [[ -n ${LEDGER:-} && -s $GATE_DIR/corpus.tsv ]]; then
    CMP=$(python3 tests/compare_corpus.py "$LEDGER" "$GATE_DIR/corpus.tsv" 2>&1)
    print "$CMP" > "$GATE_DIR/compare.json"
    LOSSES=$(print "$CMP" | python3 -c 'import json,sys; print(json.load(sys.stdin)["losses"])' 2>/dev/null || print "?")
    CHANGES=$(print "$CMP" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["changes"]))' 2>/dev/null || print "?")
    print "  vs $LEDGER: losses=$LOSSES changes=$CHANGES"
    [[ $LOSSES == 0 ]] || FAIL+=("corpus losses=$LOSSES vs $LEDGER")
  else
    FAIL+=("corpus has no ledger to compare against (ledger='${LEDGER:-}', tsv=$GATE_DIR/corpus.tsv)")
  fi
fi

# --- hygiene ----------------------------------------------------------------
step "fmt"
cargo fmt --all --check > "$GATE_DIR/fmt.log" 2>&1 || FAIL+=("cargo fmt --check")

print "\n=== summary"
print "  HEAD=$HEAD  logs=$GATE_DIR"
for n in ${NOTE[@]:-}; do print "  note: $n"; done
if [[ ${#FAIL[@]} -eq 0 ]]; then
  print "VERDICT=PASS"
else
  for f in ${FAIL[@]}; do print "  fail: $f"; done
  print "VERDICT=FAIL"
fi
print "DONE"
# Keep the completion sentinel on both paths, then propagate the verdict to
# callers that also check the process status.
(( ${#FAIL[@]} == 0 ))
