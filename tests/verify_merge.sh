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
# The five heavy steps (slick_subset, the cats and gitbucket execution
# harnesses, the workspace suite and the full corpus) run CONCURRENTLY -- see
# "the parallel block" below -- and the summary carries a per-step wall-time
# table, so nobody has to derive the cost of a step from log mtimes again.
#
# Env:
#   GATE_DIR      where every log goes (default: a fresh per-invocation dir)
#   GATE_LEDGER   corpus baseline to compare against (default: ledger named by
#                 the current row in tests/BASELINE.md)
#   GATE_SKIP     space-separated heavy steps to skip, e.g. "corpus". A skipped
#                 required check produces VERDICT=FAIL; it can never read as a
#                 passing merge gate.
#   GATE_SERIAL=1 run the heavy steps one after another instead of
#                 concurrently. Same checks, same numbers, ~2.5x the wall
#                 time; keep it as the way to reproduce a confusing result
#                 without the concurrency in the picture.
#   GATE_TEST_JOBS / GATE_TEST_THREADS
#                 how the workspace suite is fanned out: test binaries in
#                 flight (default 6) x RUST_TEST_THREADS inside each (default
#                 4). `.cargo/config.toml` pins the *interactive*
#                 RUST_TEST_THREADS to 6 so a slice's `cargo test` leaves the
#                 machine usable; that default is not touched -- the gate owns
#                 the machine and sets the value for its own run only.
#                 Raising RUST_TEST_THREADS alone is worth nothing here: the
#                 per-binary times libtest reports summed to 1862 s at 6 and
#                 1850 s at 12, because `cargo test` runs one binary at a time
#                 and most binaries hold too few tests to use the threads.
#                 tests/workspace_tests.sh runs the binaries themselves in
#                 parallel, which is where the time actually was.
#   GATE_CORPUS_JOBS    CORPUS_JOBS for the gate's corpus run (default 10;
#                 an explicit CORPUS_JOBS in the environment wins).
#   GATE_SUBSET_JOBS    javap fan-out inside tests/slick_subset.sh (default 6).
#   GATE_SKIP="cats_run gb_run"
#                 skips the two differential *execution* harnesses for cats and
#                 gitbucket (tests/cats_run.sh, tests/gitbucket_run.sh). They are
#                 the only checks that run a single instruction of either
#                 project; the compile measures above say nothing about whether
#                 what they emit works. Both are well under a minute.
#   GATE_CORPUS_TIMEOUT / GATE_CORPUS_RUN_TIMEOUT
#                 the corpus per-compile / per-`java Test` limits for the
#                 gate's run (defaults 120 / 60 against the harness defaults
#                 of 40 / 20). THIS IS LOAD-CRITICAL, not a convenience: a
#                 corpus compile that exceeds its limit is recorded as `skip`,
#                 and `compare_corpus.py` counts a pass that became a skip as
#                 a LOSS. Running the corpus next to `cargo test` makes both
#                 slower, so the limits have to cover the slowest honest run
#                 or the gate invents losses. Raising them cannot hide a
#                 regression: a higher limit only ever turns `skip` into
#                 `pass` or `fail`, never a `pass` into a `skip`.
set -u
zmodload -i zsh/datetime        # EPOCHREALTIME, for the step-time table
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
cd "$ROOT"
source "$ROOT/tests/gate_result.sh"
source "$ROOT/tests/baseline_invariants.sh"
GATE_DIR=${GATE_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/scala-rs-gate-XXXXXX")}
mkdir -p "$GATE_DIR"

# A caller may intentionally reuse GATE_DIR to keep all evidence in one place.
# Serialize those runs before touching any evidence, and recover only a lock
# whose recorded owner is demonstrably gone. A lock with no valid owner is
# treated as busy: removing it could erase a live run's results.
GATE_LOCK=$GATE_DIR/.gate.lock
gate_lock_failed() {
  print "gate: GATE_DIR is locked by another or an unverifiable run: $GATE_DIR" >&2
  print "VERDICT=FAIL"
  print "DONE"
  exit 1
}
gate_setup_failed() {
  print "gate: cannot prepare exclusive GATE_DIR: $GATE_DIR" >&2
  print "VERDICT=FAIL"
  print "DONE"
  exit 1
}
if ! mkdir "$GATE_LOCK" 2>/dev/null; then
  GATE_OWNER=$(cat "$GATE_LOCK/pid" 2>/dev/null || print -- "")
  if [[ $GATE_OWNER != <-> ]] || kill -0 "$GATE_OWNER" 2>/dev/null; then
    gate_lock_failed
  fi
  rmdir "$GATE_LOCK" 2>/dev/null || gate_lock_failed
  mkdir "$GATE_LOCK" 2>/dev/null || gate_lock_failed
fi
print -r -- "$$" > "$GATE_LOCK/pid" || gate_lock_failed
trap 'rm -f "$GATE_LOCK/pid"; rmdir "$GATE_LOCK" 2>/dev/null' EXIT

# Clear only files/directories this script owns, after the lock is held. This
# prevents a reused directory from satisfying checks with a prior run's logs,
# corpus TSV, timing files, or structured records.
STEP_ORDER=(build m_slick m_cats m_gitbucket m_library slick_run subset cats_run gb_run tests corpus corpus_compare fmt)
TDIR=$GATE_DIR/steps
mkdir -p "$TDIR" || gate_setup_failed
GATE_CLEAN_RC=0
for step_name in $STEP_ORDER; do
  rm -f "$TDIR/$step_name.result.json" "$TDIR/$step_name.rc" "$TDIR/$step_name.secs" || GATE_CLEAN_RC=$?
done
for owned_file in build.log m_slick.log m_cats.log m_gitbucket.log m_library.log \
                  slick_run.log subset.log cats_run.log gb_run.log tests.log \
                  corpus.log corpus.tsv corpus.tsv.part compare.json gate-result.json \
                  scala-rs; do
  rm -f "$GATE_DIR/$owned_file" || GATE_CLEAN_RC=$?
done
for stale_retry in "$GATE_DIR"/retry-*.tsv(N) "$GATE_DIR"/retry-*.log(N); do
  rm -f "$stale_retry" || GATE_CLEAN_RC=$?
done
rm -rf "$GATE_DIR/wt" || GATE_CLEAN_RC=$?
(( GATE_CLEAN_RC == 0 )) || gate_setup_failed

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
  # The ledger of the commit `tests/BASELINE.md` names in its header table --
  # that is the accepted baseline. Picking the first ledger *mentioned* in the
  # file chose a ledger one or more gates old (the header is followed by the
  # history of every gate), so the gate compared against stale data and its
  # "losses" were noise.
  # Never fall back to the first historical link. If the current header is
  # malformed or its ledger is absent, leave this unresolved so the corpus
  # step fails closed instead of comparing against an old accepted gate.
  LEDGER=""
  LEDGER=$(read_corpus_baseline) || LEDGER=""
fi
# The corpus checkout the runner defaults to has been found gutted (a `.git`
# with no HEAD or refs), and `scala_corpus.sh` then dies on `git rev-parse`
# leaving no tsv at all -- which the gate reported as "no ledger to compare
# against", pointing at the wrong thing. Check it here and fall back to the
# known-good checkout, saying so.
if [[ -z ${CORPUS_DIR:-} ]]; then
  for c in /tmp/scala-rs-corpus/scala /private/tmp/scala-rs-corpus-20260910-codex; do
    if git -C $c rev-parse HEAD >/dev/null 2>&1; then export CORPUS_DIR=$c; break; fi
  done
  if [[ -z ${CORPUS_DIR:-} ]]; then
    print "gate: no usable corpus checkout (tried /tmp/scala-rs-corpus/scala and /private/tmp/scala-rs-corpus-20260910-codex)"
  elif [[ $CORPUS_DIR != /tmp/scala-rs-corpus/scala ]]; then
    print "gate: default corpus checkout unusable, using CORPUS_DIR=$CORPUS_DIR"
  fi
fi
SKIP=${GATE_SKIP:-}
skipped() { [[ " $SKIP " == *" $1 "* ]] }

SERIAL=${GATE_SERIAL:-0}
TEST_JOBS=${GATE_TEST_JOBS:-6}
TEST_THREADS=${GATE_TEST_THREADS:-4}
CORPUS_J=${GATE_CORPUS_JOBS:-${CORPUS_JOBS:-10}}
SUBSET_J=${GATE_SUBSET_JOBS:-6}
CORPUS_TMO=${GATE_CORPUS_TIMEOUT:-${CORPUS_TIMEOUT:-120}}
CORPUS_RTMO=${GATE_CORPUS_RUN_TIMEOUT:-${CORPUS_RUN_TIMEOUT:-60}}

HEAD=$(git rev-parse --short=8 HEAD)
DIRTY=$(git status --porcelain | wc -l | tr -d ' ')
print "gate: HEAD=$HEAD dirty_files=$DIRTY dir=$GATE_DIR ledger=${LEDGER:-none}"
print "gate: serial=$SERIAL test_jobs=${TEST_JOBS}x${TEST_THREADS} corpus_jobs=$CORPUS_J subset_jobs=$SUBSET_J corpus_timeout=$CORPUS_TMO/$CORPUS_RTMO"
if [[ -n ${LEDGER:-} && ! -s $LEDGER ]]; then
  print "gate: ledger $LEDGER does not exist or is empty"
  LEDGER=""
fi

FAIL=()
NOTE=()

step() { print "\n=== $1"; }

# --- step timing ------------------------------------------------------------
# Every timed step writes `<name>.secs` and `<name>.rc` into $TDIR, whether it
# ran in the foreground or as one of the concurrent jobs, so the table below
# is built the same way in both modes.
# The table is printed in this order; a step with no `.secs` file (skipped, or
# never reached) is left out. A fixed list rather than one appended to as the
# gate runs; each step writes its own result record so shell output parsing never
# has to carry an exit status through a command-substitution pipeline.
timed() {  # name, command...
  local name=$1; shift
  local t0=$EPOCHREALTIME rc=0
  "$@" || rc=$?
  printf '%.1f\n' $(( EPOCHREALTIME - t0 )) > "$TDIR/$name.secs"
  print -r -- $rc > "$TDIR/$name.rc"
  return $rc
}

last_nonempty_line() {
  local log_file=$1
  awk 'NF { line=$0 } END { if (line != "") print line }' "$log_file" 2>/dev/null
}

summary_for() {
  local name=$1 log_file=$2
  case $name in
    subset)
      # The subset contract is three non-phase lines: loader, lint, counts.
      awk '!/^phase / { lines[++n]=$0 }
        END {
          first=n-2; if (first < 1) first=1
          for (i=first; i<=n; i++) { if (i > first) printf " "; printf "%s", lines[i] }
          if (n) print ""
        }' "$log_file" 2>/dev/null
      ;;
    cats_run|gb_run)
      awk '/^progs=/ { line=$0 } END { if (line != "") print line }' "$log_file" 2>/dev/null
      ;;
    tests)
      awk '/^workspace_tests: binaries=/ { line=$0 } END { if (line != "") print line }' "$log_file" 2>/dev/null
      ;;
    corpus)
      awk '/^(pos|neg|run): total/ { line=$0 } END { if (line != "") print line }' "$log_file" 2>/dev/null
      ;;
    *)
      last_nonempty_line "$log_file"
      ;;
  esac
}

run_timed_capture() {  # name, log, command...
  local name=$1 log_file=$2; shift 2
  local rc=0 summary result_status=pass
  timed "$name" "$@" > "$log_file" 2>&1 || rc=$?
  summary=$(summary_for "$name" "$log_file")
  (( rc == 0 )) || result_status=fail
  if ! gate_result_write "$TDIR/$name.result.json" "$name" "$result_status" "$rc" "$summary"; then
    print "gate: could not write structured result for $name" >&2
    return 125
  fi
  return $rc
}

require_result() {
  local name=$1 result_path="$TDIR/$1.result.json"
  if ! gate_result_read "$result_path" "$name"; then
    FAIL+=("$name result missing or malformed: $result_path")
    return 1
  fi
  if [[ $GATE_RESULT_STATUS != pass || $GATE_RESULT_RC != 0 ]]; then
    FAIL+=("$name result status=$GATE_RESULT_STATUS exit_code=$GATE_RESULT_RC")
    return 1
  fi
  return 0
}

spawn() {  # name, log, command... -- same bookkeeping, in the background
  local name=$1; shift
  local log_file=$1; shift
  ( t0=$EPOCHREALTIME; rc=0
    "$@" > "$log_file" 2>&1 || rc=$?
    printf '%.1f\n' $(( EPOCHREALTIME - t0 )) > "$TDIR/$name.secs"
    print -r -- $rc > "$TDIR/$name.rc"
    summary=$(summary_for "$name" "$log_file")
    result_status=pass
    (( rc == 0 )) || result_status=fail
    gate_result_write "$TDIR/$name.result.json" "$name" "$result_status" "$rc" "$summary" ||
      print "gate: could not write structured result for $name" >&2
  ) &
}

secs_of() { cat "$TDIR/$1.secs" 2>/dev/null || print -- "?" }
rc_of()   { cat "$TDIR/$1.rc"   2>/dev/null || print -- "?" }

# A step whose script died but whose summary line still happened to satisfy the
# text checks used to read as a pass. `harness_ok` is the backstop: if a step
# exited non-zero and contributed no failure of its own, that is a harness
# error and it is named. (In the serial gate this hole was real too: every
# heavy step was run through a summary pipeline, which discarded the status.)
harness_ok() {  # name, FAIL count taken before the step's own checks
  local name=$1 before=$2 rc=$(rc_of $1)
  (( ${#FAIL[@]} > before )) && return
  [[ $rc == 0 ]] || FAIL+=("$name exited $rc with no failing check (harness error?)")
}

GATE_T0=$EPOCHREALTIME

finish_gate() {
  local verdict=PASS gate_status=pass rc=0
  if (( ${#FAIL[@]} != 0 )); then
    verdict=FAIL; gate_status=fail; rc=1
  fi
  if ! gate_result_write "$GATE_DIR/gate-result.json" gate "$gate_status" "$rc" "verdict=$verdict failures=${#FAIL[@]}"; then
    print "gate: could not write final structured result" >&2
    verdict=FAIL; rc=1
  fi
  print "VERDICT=$verdict"
  print "DONE"
  return $rc
}

step "build"
BUILD_RC=0
run_timed_capture build "$GATE_DIR/build.log" cargo build --release -p scala-rs-cli || BUILD_RC=$?
if (( BUILD_RC != 0 )); then
  print "BUILD FAILED"; tail -30 "$GATE_DIR/build.log"
  FAIL+=("build exited $BUILD_RC")
  finish_gate; exit $?
fi

# The binary the concurrent steps run. `cargo test --workspace --release` may
# relink `target/release/scala-rs` (workspace feature unification differs from
# `-p scala-rs-cli`), and a corpus worker that execs it mid-relink fails for a
# reason that has nothing to do with the tree. So the heavy steps get an
# immutable snapshot of the binary we just built, handed to them as $SCALA_RS.
GATE_BIN=$GATE_DIR/scala-rs
if ! cp target/release/scala-rs "$GATE_BIN"; then
  FAIL+=("could not snapshot target/release/scala-rs")
  finish_gate; exit $?
fi

# --- compile measures -------------------------------------------------------
# Each script validates its own result; we keep the summary line and check the
# few invariants that are not allowed to move.
#
# These stay serial on purpose: they are seconds each (gitbucket excepted,
# which is macro-bound), and a measure that competes with another measure --
# or with itself -- is no longer a measurement of anything.
step "compile measures"
MRC=0; run_timed_capture m_slick "$GATE_DIR/m_slick.log" env SCALA_RS="$GATE_BIN" SLICK_LOG=$GATE_DIR/slick.txt tests/slick_measure.sh || MRC=$?
if require_result m_slick; then SLICK=$GATE_RESULT_SUMMARY; else SLICK=""; fi
print "  slick     $SLICK"
MRC=0; run_timed_capture m_cats "$GATE_DIR/m_cats.log" env SCALA_RS="$GATE_BIN" CATS_LOG=$GATE_DIR/cats.txt tests/cats_measure.sh || MRC=$?
if require_result m_cats; then CATS=$GATE_RESULT_SUMMARY; else CATS=""; fi
print "  cats      $CATS"
MRC=0; run_timed_capture m_gitbucket "$GATE_DIR/m_gitbucket.log" env SCALA_RS="$GATE_BIN" GITBUCKET_LOG=$GATE_DIR/gitbucket.txt tests/gitbucket_measure.sh || MRC=$?
if require_result m_gitbucket; then GB=$GATE_RESULT_SUMMARY; else GB=""; fi
print "  gitbucket $GB"
MRC=0; run_timed_capture m_library "$GATE_DIR/m_library.log" env SCALA_RS="$GATE_BIN" SCALALIB_LOG=$GATE_DIR/scalalib.txt tests/scalalib_measure.sh || MRC=$?
if require_result m_library; then LIB=$GATE_RESULT_SUMMARY; else LIB=""; fi
print "  library   $LIB"

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
  local got got_rc=0
  got=$(gate_measure_field "$line" files) || got_rc=$?
  if (( got_rc != 0 )); then
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
SCALALIB_BASELINE_RC=0
SCALALIB_EXPECTED=$(read_scalalib_baseline) || SCALALIB_BASELINE_RC=$?
if (( SCALALIB_BASELINE_RC != 0 )); then
  FAIL+=("scalalib baseline invariant is missing or malformed in tests/BASELINE.md")
else
  IFS=$'\t' read -r SCALALIB_EXPECTED_ERRORS SCALALIB_EXPECTED_FILES <<< "$SCALALIB_EXPECTED"
  SCALALIB_ERRORS_RC=0; SCALALIB_ERRORS=$(gate_measure_field "$LIB" errors) || SCALALIB_ERRORS_RC=$?
  SCALALIB_BADFILES_RC=0; SCALALIB_BADFILES=$(gate_measure_field "$LIB" files_with_errors) || SCALALIB_BADFILES_RC=$?
  if (( SCALALIB_ERRORS_RC != 0 || SCALALIB_BADFILES_RC != 0 )); then
    FAIL+=("scalalib measure has malformed errors/files_with_errors fields: $LIB")
  else
    [[ $SCALALIB_ERRORS == $SCALALIB_EXPECTED_ERRORS ]] || \
      FAIL+=("scalalib errors=$SCALALIB_ERRORS, baseline requires $SCALALIB_EXPECTED_ERRORS: $LIB")
    [[ $SCALALIB_BADFILES == $SCALALIB_EXPECTED_FILES ]] || \
      FAIL+=("scalalib files_with_errors=$SCALALIB_BADFILES, baseline requires $SCALALIB_EXPECTED_FILES: $LIB")
  fi
fi
[[ $SLICK == *"errors=0 files_with_errors=0 classes=1504"* ]] || FAIL+=("slick measure: $SLICK")
# cats compiles clean since `agent/catszero` (quasiquote patterns made the
# last held-out file compile, so there is no holdout any more). Zero is now
# an invariant: a regression here is a gate failure, not a number to report.
[[ $CATS == *"errors=0 files_with_errors=0"* ]] || FAIL+=("cats measure: $CATS")

# --- execution --------------------------------------------------------------
# slick_run.sh stays serial as well. It is a *differential execution* test:
# it runs each client program three times per side and compares stdout byte
# for byte, and its own header records how a shared work directory once turned
# a harness collision into something that read exactly like a compiler bug.
# Timing noise and machine load are part of what it measures, so it gets the
# machine to itself.
step "slick execution"
RRC=0; run_timed_capture slick_run "$GATE_DIR/slick_run.log" env SCALA_RS="$GATE_BIN" MODE=b RUNS=3 tests/slick_run.sh || RRC=$?
if require_result slick_run; then RUN=$GATE_RESULT_SUMMARY; else RUN=""; fi
print "  $RUN"
[[ $RUN == *"ok=12 diff=0 fail=0"* ]] || FAIL+=("slick_run: $RUN")

# --- the parallel block -----------------------------------------------------
# slick_subset, the workspace suite and the full corpus share no state:
#   * slick_subset works in $SP/subset-$$ (per invocation) and reads the
#     measure log above read-only;
#   * scala_corpus works in $CORPUS_WORK=$SP/work-$$ and writes
#     $GATE_DIR/corpus.tsv{,.part};
#   * cargo test owns target/ -- and is the reason the other two are handed
#     $GATE_BIN rather than target/release/scala-rs.
# The corpus is additionally told SCALA_RS_PREBUILT=1 so it does not reach for
# cargo (and therefore the target-directory lock) while `cargo test` holds it.
#
# Each job's stdout+stderr, exit status and wall time land in files; the checks
# below are applied afterwards in the old order, so the summary reads the same.
run_subset() {
  SLICK_SEED_LOG=$GATE_DIR/slick.txt SCALA_RS=$GATE_BIN SUBSET_JOBS=$SUBSET_J \
    tests/slick_subset.sh
}
run_tests() {
  WT_JOBS=$TEST_JOBS WT_THREADS=$TEST_THREADS WT_NO_DOC=0 WT_DIR=$GATE_DIR/wt \
    tests/workspace_tests.sh
}
run_corpus() {
  CORPUS_SIZE=full CORPUS_LOG=$GATE_DIR/corpus.tsv CORPUS_JOBS=$CORPUS_J \
  CORPUS_TIMEOUT=$CORPUS_TMO CORPUS_RUN_TIMEOUT=$CORPUS_RTMO \
  SCALA_RS=$GATE_BIN SCALA_RS_PREBUILT=1 \
    tests/scala_corpus.sh
}
# The two differential *execution* harnesses for cats and gitbucket. They belong
# in this block for the same reason `slick_subset` does: each one recompiles its
# project with `$GATE_BIN` into its own private, locked work area, reads nothing
# the others write, and takes well under a minute (cats ~32 s, gitbucket ~45 s
# with the shared scalac reference build in place -- both are timed in the table
# below, so a regression in their own cost is visible).
#
# Unlike `slick_run`, neither measures timing or load, so neither needs the
# machine to itself: they compare stdout byte for byte and they compile the
# clients with real scalac.
run_cats_run() {
  SCALA_RS=$GATE_BIN CATSRUN_JOBS=2 PICKLE=1 KNOWN='' tests/cats_run.sh
}
run_gb_run() {
  SCALA_RS=$GATE_BIN GBRUN_JOBS=2 PICKLE=1 KNOWN='' tests/gitbucket_run.sh
}

do_subset=1; do_tests=1; do_corpus=1; do_cats_run=1; do_gb_run=1
for requested_skip in ${=SKIP}; do
  case $requested_skip in
    subset|tests|corpus|cats_run|gb_run) ;;
    *) FAIL+=("unknown GATE_SKIP step: $requested_skip") ;;
  esac
done
if skipped subset; then
  do_subset=0; NOTE+=("slick_subset SKIPPED")
  gate_result_write "$TDIR/subset.result.json" subset skip 0 "skipped via GATE_SKIP" || FAIL+=("subset result write failed")
  FAIL+=("required step skipped: subset")
fi
if skipped tests; then
  do_tests=0; NOTE+=("workspace tests SKIPPED")
  gate_result_write "$TDIR/tests.result.json" tests skip 0 "skipped via GATE_SKIP" || FAIL+=("tests result write failed")
  FAIL+=("required step skipped: tests")
fi
if skipped corpus; then
  do_corpus=0; NOTE+=("corpus SKIPPED")
  gate_result_write "$TDIR/corpus.result.json" corpus skip 0 "skipped via GATE_SKIP" || FAIL+=("corpus result write failed")
  FAIL+=("required step skipped: corpus")
fi
if skipped cats_run; then
  do_cats_run=0; NOTE+=("cats_run SKIPPED")
  gate_result_write "$TDIR/cats_run.result.json" cats_run skip 0 "skipped via GATE_SKIP" || FAIL+=("cats_run result write failed")
  FAIL+=("required step skipped: cats_run")
fi
if skipped gb_run; then
  do_gb_run=0; NOTE+=("gitbucket_run SKIPPED")
  gate_result_write "$TDIR/gb_run.result.json" gb_run skip 0 "skipped via GATE_SKIP" || FAIL+=("gb_run result write failed")
  FAIL+=("required step skipped: gb_run")
fi

if [[ $SERIAL == 1 ]]; then
  step "heavy steps (serial, GATE_SERIAL=1)"
  if (( do_subset )); then run_timed_capture subset "$GATE_DIR/subset.log" run_subset || STEP_RC=$?; fi
  if (( do_cats_run )); then run_timed_capture cats_run "$GATE_DIR/cats_run.log" run_cats_run || STEP_RC=$?; fi
  if (( do_gb_run )); then run_timed_capture gb_run "$GATE_DIR/gb_run.log" run_gb_run || STEP_RC=$?; fi
  if (( do_tests )); then run_timed_capture tests "$GATE_DIR/tests.log" run_tests || STEP_RC=$?; fi
  if (( do_corpus )); then run_timed_capture corpus "$GATE_DIR/corpus.log" run_corpus || STEP_RC=$?; fi
else
  step "heavy steps (concurrent): slick_subset + cats_run + gitbucket_run + workspace tests + corpus"
  print "  launched; the logs are subset.log, cats_run.log, gb_run.log, tests.log, corpus.log in $GATE_DIR"
  (( do_subset ))   && spawn subset   "$GATE_DIR/subset.log" run_subset
  (( do_cats_run )) && spawn cats_run "$GATE_DIR/cats_run.log" run_cats_run
  (( do_gb_run ))   && spawn gb_run   "$GATE_DIR/gb_run.log" run_gb_run
  (( do_tests ))    && spawn tests    "$GATE_DIR/tests.log" run_tests
  (( do_corpus ))   && spawn corpus   "$GATE_DIR/corpus.log" run_corpus
  wait
fi

if (( do_subset )); then
  step "slick subset + class validation"
  NF=${#FAIL[@]}
  if require_result subset; then SUB=$GATE_RESULT_SUMMARY; else SUB=""; fi
  print "  $SUB"
  [[ $SUB == *"verified=1504 failed=0"* && $SUB == *"lint_problems=0"* ]] || FAIL+=("slick_subset: $SUB")
  harness_ok subset $NF
fi

# --- cats / gitbucket execution --------------------------------------------
# `new=0` is the check, not `fail=0`: each harness keeps a ledger of the programs
# that do not pass yet with the root that stops each one, prints every entry, and
# counts a failure that is *not* on the ledger -- or a ledger entry that now
# passes -- as `new`. So a fix that lands elsewhere and unblocks one of them
# fails this step until the ledger is pruned, and a new break fails it at once.
for h in cats_run gb_run; do
  (( ${(P)${:-do_$h}} )) || continue
  case $h in
    cats_run) what="cats execution (tests/cats_run.sh)"; log=$GATE_DIR/cats_run.log;;
    gb_run)   what="gitbucket execution (tests/gitbucket_run.sh)"; log=$GATE_DIR/gb_run.log;;
  esac
  step "$what"
  NF=${#FAIL[@]}
  grep '^   known-fail ' "$log" | sed 's/^ */  /'
  if require_result "$h"; then H=$GATE_RESULT_SUMMARY; else H=""; fi
  print "  ${H:-(no summary line)}"
  if [[ -z $H ]]; then
    FAIL+=("$h printed no summary line; see $log")
  else
    [[ $H == *" new=0 "* ]] || FAIL+=("$h: $H")
    [[ $H == *"lint_problems=0"* ]] || FAIL+=("$h classfile lint: $H")
  fi
  harness_ok $h $NF
done

# --- workspace suite --------------------------------------------------------
if (( do_tests )); then
  step "workspace tests (tests/workspace_tests.sh, --release)"
  NF=${#FAIL[@]}
  TEST_COUNTS_RC=0
  TEST_COUNTS=$(gate_workspace_counts "$GATE_DIR/tests.log") || TEST_COUNTS_RC=$?
  if (( TEST_COUNTS_RC == 0 )); then
    IFS=$'\t' read -r TEST_ROWS TEST_PASSED TEST_FAILED <<< "$TEST_COUNTS"
    T="$TEST_ROWS rows, $TEST_PASSED passed, $TEST_FAILED failed"
  else
    TEST_ROWS=0; TEST_PASSED=0; TEST_FAILED=0
    T="0 rows, 0 passed, 0 failed"
    FAIL+=("workspace tests: malformed or missing test result rows")
  fi
  print "  $T"
  NFAIL=$TEST_FAILED
  if [[ $NFAIL -ne 0 ]]; then
    FAIL+=("workspace tests: $T")
    print "  failing:"; grep -A8 '^failures:' "$GATE_DIR/tests.log" | grep -E '^    [a-z_0-9]+' | sort -u | sed 's/^/   /'
  fi
  # Summing the `test result:` lines cannot notice a binary that printed none.
  # The runner counts the binaries it launched and reports how many reported
  # nothing back, which is the check the old `cargo test | grep` never had.
  WT=$(grep -m1 '^workspace_tests: binaries=' "$GATE_DIR/tests.log")
  print "  $WT"
  TRC=0; require_result tests >/dev/null || TRC=$?
  if [[ -z $WT ]]; then
    FAIL+=("workspace tests: runner printed no summary line")
  else
    WT_DOC_RC=0; WT_DOC_ROWS=$(gate_measure_field "$WT" doc_rows) || WT_DOC_RC=$?
    if [[ $WT != *" missing=0 "* || $WT != *" failed_bins=0 "* || $WT_DOC_ROWS != 7 ]] || (( WT_DOC_RC != 0 )); then
      FAIL+=("workspace tests: $WT")
      grep '^workspace_tests: \(MISSING\|NONZERO\)' "$GATE_DIR/tests.log" | sed 's/^/   /'
    fi
  fi
  harness_ok tests $NF
fi

# --- corpus -----------------------------------------------------------------
if (( do_corpus )); then
  step "scala/scala corpus (full)"
  NF=${#FAIL[@]}
  CRC=0; require_result corpus >/dev/null || CRC=$?
  grep -E '^(pos|neg|run): total' "$GATE_DIR/corpus.log" | sed 's/^/  /'
  # How many rows were lost to a clock rather than to a compiler. Not a failure
  # by itself (a pass that became a timeout is already a loss below), but it is
  # the number to look at when the gate ran next to something else: if it is not
  # tiny, the corpus was racing for the machine and the run is suspect.
  # `grep -c` prints its count and exits 1 when that count is zero, so no `||`
  # fallback here: one would print a second line.
  TMOUT_ROWS=$(grep -cE $'\t'"skip"$'\t'"(run-)?timeout" "$GATE_DIR/corpus.tsv" 2>/dev/null)
  print "  rows skipped on a timeout: ${TMOUT_ROWS:-?} (limits ${CORPUS_TMO}s compile / ${CORPUS_RTMO}s run)"
  if [[ -n ${LEDGER:-} && -s $GATE_DIR/corpus.tsv ]]; then
    CMP_RC=0
    CMP=$(python3 tests/compare_corpus.py "$LEDGER" "$GATE_DIR/corpus.tsv" 2>&1) || CMP_RC=$?
    print "$CMP" > "$GATE_DIR/compare.json"
    CMP_PARSE_RC=0
    CMP_PARSE=$(PYTHONPATH="$ROOT/tests${PYTHONPATH:+:$PYTHONPATH}" python3 -c 'import json,sys
from compare_corpus import validate_result
try:
 result=json.load(sys.stdin)
 validate_result(result)
 print("{}\t{}".format(result["losses"], len(result["changes"])))
except (ValueError,TypeError,KeyError,json.JSONDecodeError):
 raise SystemExit(2)' <<< "$CMP" 2>/dev/null) || CMP_PARSE_RC=$?
    if (( CMP_PARSE_RC != 0 )); then
      FAIL+=("corpus comparison failed or returned malformed JSON (compare_rc=$CMP_RC parse_rc=$CMP_PARSE_RC)")
      LOSSES="?"; CHANGES="?"
      gate_result_write "$TDIR/corpus_compare.result.json" corpus_compare error 2 \
        "comparison unavailable compare_rc=$CMP_RC parse_rc=$CMP_PARSE_RC" || \
        FAIL+=("corpus comparison result write failed")
    else
      IFS=$'\t' read -r LOSSES CHANGES <<< "$CMP_PARSE"
      EXPECTED_CMP_RC=0
      (( LOSSES != 0 )) && EXPECTED_CMP_RC=1
      COMPARE_STATUS=pass; COMPARE_RESULT_RC=$CMP_RC
      if (( CMP_RC != EXPECTED_CMP_RC )); then
        FAIL+=("corpus comparison exit code $CMP_RC is inconsistent with losses=$LOSSES")
        COMPARE_STATUS=error
        (( COMPARE_RESULT_RC == 0 )) && COMPARE_RESULT_RC=2
      elif (( LOSSES != 0 )); then
        COMPARE_STATUS=fail
      fi
      gate_result_write "$TDIR/corpus_compare.result.json" corpus_compare \
        "$COMPARE_STATUS" "$COMPARE_RESULT_RC" "losses=$LOSSES changes=$CHANGES" || \
        FAIL+=("corpus comparison result write failed")
    fi
    print "  vs $LEDGER: losses=${LOSSES:-?} changes=${CHANGES:-?}"
    # A loss is classified by a *serial* re-run of just that test. Some corpus
    # tests assert their own wall-clock time (`run/t5857`: "it should be less
    # than, say, 250ms"), so on a machine running three heavy steps at once
    # they fail for the load, not for the compiler. The retry identifies that
    # condition for follow-up, but it can never turn a candidate loss green:
    # the merge gate remains FAIL until the full candidate ledger is clean.
    if [[ $LOSSES != 0 && $LOSSES != "?" ]]; then
      LOSTSPECS_RC=0
      LOSTSPECS=$(python3 -c 'import json,sys
d=json.load(sys.stdin)
print(" ".join(c["kind"]+"/"+c["test"] for c in d["changes"] if c.get("loss")))' 2>/dev/null <<< "$CMP") || LOSTSPECS_RC=$?
      if (( LOSTSPECS_RC != 0 )); then
        FAIL+=("corpus comparison changes could not be parsed (rc=$LOSTSPECS_RC)")
        LOSTSPECS=""
      fi
      print "  re-running each loss serially: $LOSTSPECS"
      STILL=()
      for spec in ${=LOSTSPECS}; do
        kind=${spec%%/*}; name=${spec#*/}
        rlog=$GATE_DIR/retry-$kind-$name.tsv
        CORPUS_KINDS=$kind CORPUS_SIZE=full CORPUS_JOBS=1 \
          CORPUS_FILTER="$name" CORPUS_LOG=$rlog CORPUS_NO_REPORT=1 \
          SCALA_RS=$GATE_BIN SCALA_RS_PREBUILT=1 tests/scala_corpus.sh \
          > $GATE_DIR/retry-$kind-$name.log 2>&1
        st=$(awk -F'\t' -v n="$name" '$2==n {print $3}' $rlog 2>/dev/null | head -1)
        print "    $spec -> ${st:-no-row}"
        [[ $st == pass ]] || STILL+=("$spec(${st:-no-row})")
      done
      if (( ${#STILL[@]} )); then
        FAIL+=("corpus losses=${#STILL[@]} vs $LEDGER: ${STILL[*]}")
      else
        FAIL+=("corpus losses=$LOSSES vs $LEDGER (all serial retries passed; load-sensitive, gate remains FAIL): $LOSTSPECS")
      fi
    fi
  else
    FAIL+=("corpus has no ledger to compare against (ledger='${LEDGER:-}', tsv=$GATE_DIR/corpus.tsv)")
    gate_result_write "$TDIR/corpus_compare.result.json" corpus_compare error 2 \
      "comparison unavailable ledger=${LEDGER:-none} tsv=$GATE_DIR/corpus.tsv" || \
      FAIL+=("corpus comparison result write failed")
  fi
  harness_ok corpus $NF
fi

# --- hygiene ----------------------------------------------------------------
step "fmt"
FRC=0; run_timed_capture fmt "$GATE_DIR/fmt.log" cargo fmt --all --check || FRC=$?
require_result fmt >/dev/null || FRC=$?

GATE_SECS=$(printf '%.0f' $(( EPOCHREALTIME - GATE_T0 )))

mmss() { printf '%d:%02d' $(( ${1:-0} / 60 )) $(( ${1:-0} % 60 )) }

print "\n=== step times (wall clock)"
printf '  %-12s %8s %8s  %s\n' step seconds m:ss rc
for n in $STEP_ORDER; do
  [[ -s $TDIR/$n.secs ]] || continue
  s=$(secs_of $n)
  printf '  %-12s %8s %8s  %s\n' $n $s "$(mmss ${s%.*})" "$(rc_of $n)"
done
printf '  %-12s %8s %8s\n' TOTAL $GATE_SECS "$(mmss $GATE_SECS)"
[[ $SERIAL == 1 ]] || print "  (subset/tests/corpus overlap: their times sum to more than TOTAL)"

print "\n=== summary"
print "  HEAD=$HEAD  logs=$GATE_DIR  wall=$(mmss $GATE_SECS)"
for n in ${NOTE[@]:-}; do print "  note: $n"; done
for f in ${FAIL[@]}; do print "  fail: $f"; done
# Keep the completion sentinel on both paths, then propagate the verdict to
# callers that also check the process status. The final JSON record is written
# before either marker so a reader can validate the result after DONE.
finish_gate
