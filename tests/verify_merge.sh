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
#   GATE_LEDGER   corpus baseline to compare against (default: newest tests/baselines/corpus-*.tsv)
#   GATE_SKIP     space-separated steps to skip, e.g. "corpus" -- each skip is
#                 printed in the summary, because a skipped check must never
#                 read as a passing one.
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
  # The ledger of the commit `tests/BASELINE.md` names in its header table --
  # that is the accepted baseline. Picking the first ledger *mentioned* in the
  # file chose a ledger one or more gates old (the header is followed by the
  # history of every gate), so the gate compared against stale data and its
  # "losses" were noise.
  CUR=$(grep -m1 -oE '^\| commit \| `[0-9a-f]{8}`' tests/BASELINE.md 2>/dev/null | grep -oE '[0-9a-f]{8}')
  if [[ -n $CUR && -s tests/baselines/corpus-$CUR.tsv ]]; then
    LEDGER=tests/baselines/corpus-$CUR.tsv
  else
    LEDGER=$(grep -oE 'tests/baselines/corpus-[0-9a-f]{8}\.tsv|baselines/corpus-[0-9a-f]{8}\.tsv' tests/BASELINE.md 2>/dev/null \
               | sed 's|^baselines/|tests/baselines/|' | head -1)
  fi
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
TDIR=$GATE_DIR/steps
mkdir -p "$TDIR"
# The table is printed in this order; a step with no `.secs` file (skipped, or
# never reached) is left out. A fixed list rather than one appended to as the
# gate runs, because the four measures are timed inside a command substitution
# and a subshell cannot append to its parent's array.
STEP_ORDER=(build m_slick m_cats m_gitbucket m_library slick_run subset cats_run gb_run tests corpus fmt)

timed() {  # name, command...
  local name=$1; shift
  local t0=$EPOCHREALTIME rc=0
  "$@" || rc=$?
  printf '%.1f\n' $(( EPOCHREALTIME - t0 )) > "$TDIR/$name.secs"
  print -r -- $rc > "$TDIR/$name.rc"
  return $rc
}

spawn() {  # name, command... -- same bookkeeping, in the background
  local name=$1; shift
  ( t0=$EPOCHREALTIME; rc=0
    "$@" || rc=$?
    printf '%.1f\n' $(( EPOCHREALTIME - t0 )) > "$TDIR/$name.secs"
    print -r -- $rc > "$TDIR/$name.rc"
  ) &
}

secs_of() { cat "$TDIR/$1.secs" 2>/dev/null || print -- "?" }
rc_of()   { cat "$TDIR/$1.rc"   2>/dev/null || print -- "?" }

# A step whose script died but whose summary line still happened to satisfy the
# text checks used to read as a pass. `harness_ok` is the backstop: if a step
# exited non-zero and contributed no failure of its own, that is a harness
# error and it is named. (In the serial gate this hole was real too: every
# heavy step was run through a `| tail` pipeline, which discards the status.)
harness_ok() {  # name, FAIL count taken before the step's own checks
  local name=$1 before=$2 rc=$(rc_of $1)
  (( ${#FAIL[@]} > before )) && return
  [[ $rc == 0 ]] || FAIL+=("$name exited $rc with no failing check (harness error?)")
}

GATE_T0=$EPOCHREALTIME

step "build"
if ! timed build cargo build --release -p scala-rs-cli > "$GATE_DIR/build.log" 2>&1; then
  print "BUILD FAILED"; tail -30 "$GATE_DIR/build.log"
  print "VERDICT=FAIL (build)"; print "DONE"; exit 1
fi

# The binary the concurrent steps run. `cargo test --workspace --release` may
# relink `target/release/scala-rs` (workspace feature unification differs from
# `-p scala-rs-cli`), and a corpus worker that execs it mid-relink fails for a
# reason that has nothing to do with the tree. So the heavy steps get an
# immutable snapshot of the binary we just built, handed to them as $SCALA_RS.
GATE_BIN=$GATE_DIR/scala-rs
cp target/release/scala-rs "$GATE_BIN"

# --- compile measures -------------------------------------------------------
# Each script validates its own result; we keep the summary line and check the
# few invariants that are not allowed to move.
#
# These stay serial on purpose: they are seconds each (gitbucket excepted,
# which is macro-bound), and a measure that competes with another measure --
# or with itself -- is no longer a measurement of anything.
step "compile measures"
SLICK=$(timed m_slick env SLICK_LOG=$GATE_DIR/slick.txt tests/slick_measure.sh 2>&1 | tail -1); print "  slick     $SLICK"
CATS=$(timed m_cats env CATS_LOG=$GATE_DIR/cats.txt tests/cats_measure.sh 2>&1 | tail -1);      print "  cats      $CATS"
GB=$(timed m_gitbucket env GITBUCKET_LOG=$GATE_DIR/gitbucket.txt tests/gitbucket_measure.sh 2>&1 | tail -1); print "  gitbucket $GB"
LIB=$(timed m_library env SCALALIB_LOG=$GATE_DIR/scalalib.txt tests/scalalib_measure.sh 2>&1 | tail -1);     print "  library   $LIB"

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
# slick_run.sh stays serial as well. It is a *differential execution* test:
# it runs each client program three times per side and compares stdout byte
# for byte, and its own header records how a shared work directory once turned
# a harness collision into something that read exactly like a compiler bug.
# Timing noise and machine load are part of what it measures, so it gets the
# machine to itself.
step "slick execution"
RUN=$(timed slick_run env MODE=b tests/slick_run.sh 2>&1 | tail -1); print "  $RUN"
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
    tests/slick_subset.sh > "$GATE_DIR/subset.log" 2>&1
}
run_tests() {
  WT_JOBS=$TEST_JOBS WT_THREADS=$TEST_THREADS WT_DIR=$GATE_DIR/wt \
    tests/workspace_tests.sh > "$GATE_DIR/tests.log" 2>&1
}
run_corpus() {
  CORPUS_SIZE=full CORPUS_LOG=$GATE_DIR/corpus.tsv CORPUS_JOBS=$CORPUS_J \
  CORPUS_TIMEOUT=$CORPUS_TMO CORPUS_RUN_TIMEOUT=$CORPUS_RTMO \
  SCALA_RS=$GATE_BIN SCALA_RS_PREBUILT=1 \
    tests/scala_corpus.sh > "$GATE_DIR/corpus.log" 2>&1
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
  SCALA_RS=$GATE_BIN CATSRUN_JOBS=2 tests/cats_run.sh > "$GATE_DIR/cats_run.log" 2>&1
}
run_gb_run() {
  SCALA_RS=$GATE_BIN GBRUN_JOBS=2 tests/gitbucket_run.sh > "$GATE_DIR/gb_run.log" 2>&1
}

do_subset=1; do_tests=1; do_corpus=1; do_cats_run=1; do_gb_run=1
skipped subset && { do_subset=0; NOTE+=("slick_subset SKIPPED"); }
skipped tests  && { do_tests=0;  NOTE+=("workspace tests SKIPPED"); }
skipped corpus && { do_corpus=0; NOTE+=("corpus SKIPPED"); }
skipped cats_run && { do_cats_run=0; NOTE+=("cats_run SKIPPED"); }
skipped gb_run && { do_gb_run=0; NOTE+=("gitbucket_run SKIPPED"); }

if [[ $SERIAL == 1 ]]; then
  step "heavy steps (serial, GATE_SERIAL=1)"
  (( do_subset ))   && timed subset   run_subset
  (( do_cats_run )) && timed cats_run run_cats_run
  (( do_gb_run ))   && timed gb_run   run_gb_run
  (( do_tests ))    && timed tests    run_tests
  (( do_corpus ))   && timed corpus   run_corpus
else
  step "heavy steps (concurrent): slick_subset + cats_run + gitbucket_run + workspace tests + corpus"
  print "  launched; the logs are subset.log, cats_run.log, gb_run.log, tests.log, corpus.log in $GATE_DIR"
  (( do_subset ))   && spawn subset   run_subset
  (( do_cats_run )) && spawn cats_run run_cats_run
  (( do_gb_run ))   && spawn gb_run   run_gb_run
  (( do_tests ))    && spawn tests    run_tests
  (( do_corpus ))   && spawn corpus   run_corpus
  wait
fi

if (( do_subset )); then
  step "slick subset + class validation"
  NF=${#FAIL[@]}
  # `tail -3` is the contract with slick_subset.sh: its last three lines are the
  # loader verdict, the lint verdict and the file/class counts, and when the
  # script dies early those three lines are whatever it died saying. Its phase
  # timings are the one thing that must not be counted, or they push the loader
  # verdict out of the window -- which is exactly what happened the first time.
  SUB=$(grep -v '^phase ' "$GATE_DIR/subset.log" | tail -3 | tr '\n' ' '); print "  $SUB"
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
  H=$(grep -m1 '^progs=' "$log")
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
  T=$(grep '^test result:' "$GATE_DIR/tests.log" | awk -F'[ ;]' '{p+=$4; f+=$7} END {print NR" rows, "p" passed, "f" failed"}')
  print "  $T"
  NFAIL=$(grep '^test result:' "$GATE_DIR/tests.log" | awk -F'[ ;]' '{f+=$7} END {print f+0}')
  if [[ $NFAIL -ne 0 ]]; then
    FAIL+=("workspace tests: $T")
    print "  failing:"; grep -A8 '^failures:' "$GATE_DIR/tests.log" | grep -E '^    [a-z_0-9]+' | sort -u | sed 's/^/   /'
  fi
  # Summing the `test result:` lines cannot notice a binary that printed none.
  # The runner counts the binaries it launched and reports how many reported
  # nothing back, which is the check the old `cargo test | grep` never had.
  WT=$(grep -m1 '^workspace_tests: binaries=' "$GATE_DIR/tests.log")
  print "  $WT"
  if [[ -z $WT ]]; then
    FAIL+=("workspace tests: runner printed no summary line")
  elif [[ $WT != *" missing=0 "* || $WT != *" failed_bins=0 "* ]]; then
    FAIL+=("workspace tests: $WT")
    grep '^workspace_tests: \(MISSING\|NONZERO\)' "$GATE_DIR/tests.log" | sed 's/^/   /'
  fi
  harness_ok tests $NF
fi

# --- corpus -----------------------------------------------------------------
if (( do_corpus )); then
  step "scala/scala corpus (full)"
  NF=${#FAIL[@]}
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
    CMP=$(python3 tests/compare_corpus.py "$LEDGER" "$GATE_DIR/corpus.tsv" 2>&1)
    print "$CMP" > "$GATE_DIR/compare.json"
    LOSSES=$(print "$CMP" | python3 -c 'import json,sys; print(json.load(sys.stdin)["losses"])' 2>/dev/null || print "?")
    CHANGES=$(print "$CMP" | python3 -c 'import json,sys; print(len(json.load(sys.stdin)["changes"]))' 2>/dev/null || print "?")
    print "  vs $LEDGER: losses=$LOSSES changes=$CHANGES"
    # A loss is confirmed by a *serial* re-run of just that test. Some corpus
    # tests assert their own wall-clock time (`run/t5857`: "it should be less
    # than, say, 250ms"), so on a machine running three heavy steps at once
    # they fail for the load, not for the compiler -- and a gate that invents
    # losses is as useless as one that hides them. A real regression fails
    # both times; this costs seconds and only runs when there is a loss.
    if [[ $LOSSES != 0 && $LOSSES != "?" ]]; then
      LOSTSPECS=$(print "$CMP" | python3 -c 'import json,sys
d=json.load(sys.stdin)
print(" ".join(c["kind"]+"/"+c["test"] for c in d["changes"] if c.get("loss")))' 2>/dev/null)
      print "  re-running each loss serially: $LOSTSPECS"
      STILL=()
      for spec in ${=LOSTSPECS}; do
        kind=${spec%%/*}; name=${spec#*/}
        rlog=$GATE_DIR/retry-$kind-$name.tsv
        CORPUS_KINDS=$kind CORPUS_SIZE=full CORPUS_JOBS=1 \
          CORPUS_FILTER="^${name}$" CORPUS_LOG=$rlog CORPUS_NO_REPORT=1 \
          SCALA_RS=$GATE_BIN SCALA_RS_PREBUILT=1 tests/scala_corpus.sh \
          > $GATE_DIR/retry-$kind-$name.log 2>&1
        st=$(awk -F'\t' -v n="$name" '$2==n {print $3}' $rlog 2>/dev/null | head -1)
        print "    $spec -> ${st:-no-row}"
        [[ $st == pass ]] || STILL+=("$spec(${st:-no-row})")
      done
      if (( ${#STILL[@]} )); then
        FAIL+=("corpus losses=${#STILL[@]} vs $LEDGER: ${STILL[*]}")
      else
        NOTE+=("corpus: $LOSSES loss(es) passed on a serial re-run (load-sensitive, not a regression): $LOSTSPECS")
      fi
    fi
  else
    FAIL+=("corpus has no ledger to compare against (ledger='${LEDGER:-}', tsv=$GATE_DIR/corpus.tsv)")
  fi
  harness_ok corpus $NF
fi

# --- hygiene ----------------------------------------------------------------
step "fmt"
timed fmt cargo fmt --all --check > "$GATE_DIR/fmt.log" 2>&1 || FAIL+=("cargo fmt --check")

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
