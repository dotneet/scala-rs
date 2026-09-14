#!/bin/zsh
# Contract checks for the merge gate's baseline and corpus comparison paths.
# This intentionally does not build the compiler or run any fixture project.
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=$(mktemp -d "${TMPDIR:-/tmp}/scala-rs-verify-merge-test.XXXXXX")
trap 'rm -rf "$WORK"' EXIT INT TERM

# Load just the baseline reader; sourcing the gate itself would launch the full
# merge battery.
source "$ROOT/tests/baseline_invariants.sh"

[[ $(read_scalalib_baseline "$ROOT/tests/BASELINE.md") == $'2\t2' ]] || {
  print -u2 'accepted scalalib baseline is not 2/2'
  exit 1
}
[[ $(read_corpus_baseline "$ROOT/tests/BASELINE.md" "$ROOT/tests/baselines") == "$ROOT/tests/baselines/corpus-ca3dfd6d.tsv" ]] || {
  print -u2 'current corpus baseline does not resolve to ca3dfd6d'
  exit 1
}

print -r -- '| `tests/scalalib_measure.sh` (538) | **2 (stale)** | **2** | — |' > "$WORK/malformed.md"
if read_scalalib_baseline "$WORK/malformed.md" >/dev/null 2>&1; then
  print -u2 'baseline reader accepted a decorated numeric cell'
  exit 1
fi

print -r -- '| `tests/scalalib_measure.sh` (538) | **2** | **2** | — |' > "$WORK/duplicate.md"
print -r -- '| `tests/scalalib_measure.sh` (538) | **2** | **2** | — |' >> "$WORK/duplicate.md"
if read_scalalib_baseline "$WORK/duplicate.md" >/dev/null 2>&1; then
  print -u2 'baseline reader accepted duplicate rows'
  exit 1
fi

print -r -- '| commit | `deadbeef` |' > "$WORK/missing-ledger.md"
print -r -- 'Historical link: `baselines/corpus-ca3dfd6d.tsv`' >> "$WORK/missing-ledger.md"
if read_corpus_baseline "$WORK/missing-ledger.md" "$ROOT/tests/baselines" >/dev/null 2>&1; then
  print -u2 'corpus baseline reader fell back to a historical ledger'
  exit 1
fi

# Run the gate against a tiny fake project with deliberately hostile inherited
# values. This exercises the actual command prefixes, rather than only checking
# that the gate's source happens to mention the expected defaults.
FAKE_ROOT="$WORK/fake-root"
FAKE_BIN_DIR="$WORK/fake-bin"
FAKE_GATE_DIR="$WORK/fake-gate"
FAKE_CORPUS_DIR="$WORK/fake-corpus"
ENV_LOG="$WORK/env.log"
mkdir -p "$FAKE_ROOT/tests/baselines" "$FAKE_BIN_DIR" "$FAKE_CORPUS_DIR"

for helper in verify_merge.sh gate_result.sh gate_result.py baseline_invariants.sh compare_corpus.py; do
  cp "$ROOT/tests/$helper" "$FAKE_ROOT/tests/$helper"
done
print -r -- '| `tests/scalalib_measure.sh` (538) | **2** | **2** | — |' > "$FAKE_ROOT/tests/BASELINE.md"
print -r -- $'pos\tprobe\tpass' > "$WORK/ledger.tsv"

print -r -- '#!/bin/zsh' > "$FAKE_BIN_DIR/cargo"
print -r -- 'if [[ $1 == build ]]; then mkdir -p target/release; print -r -- "fake compiler" > target/release/scala-rs; chmod +x target/release/scala-rs; exit 0; fi' >> "$FAKE_BIN_DIR/cargo"
print -r -- 'exit 0' >> "$FAKE_BIN_DIR/cargo"
chmod +x "$FAKE_BIN_DIR/cargo"

make_fake_step() {
  local name=$1 summary=$2 tag=${1%.sh} script_path="$FAKE_ROOT/tests/$1"
  print -r -- '#!/bin/zsh' > "$script_path"
  print -r -- 'print -r -- "'"$tag"' SCALA_RS=${SCALA_RS:-unset} WT_NO_DOC=${WT_NO_DOC:-unset} PICKLE=${PICKLE:-unset} RUNS=${RUNS:-unset} KNOWN=${KNOWN-unset}" >> "$ENV_LOG"' >> "$script_path"
  print -r -- "print -r -- '$summary'" >> "$script_path"
  chmod +x "$script_path"
}

make_fake_step slick_measure.sh 'slick files=184 errors=0 files_with_errors=0 classes=1504'
make_fake_step cats_measure.sh 'cats files=340 errors=0 files_with_errors=0'
make_fake_step gitbucket_measure.sh 'gitbucket files=354 java_sources=3 errors=0 files_with_errors=0'
make_fake_step scalalib_measure.sh 'library files=538 errors=2 files_with_errors=2'
make_fake_step slick_run.sh 'progs=12 ok=12 diff=0 fail=0 runs=3'
make_fake_step slick_subset.sh 'loader ok'
print -r -- 'print -r -- "lint_problems=0"' >> "$FAKE_ROOT/tests/slick_subset.sh"
print -r -- 'print -r -- "verified=1504 failed=0"' >> "$FAKE_ROOT/tests/slick_subset.sh"
make_fake_step cats_run.sh 'progs=1 ok=1 diff=0 fail=0 classes=1 pickle_fail=0 known_fail=0 new=0 lint_problems=0'
make_fake_step gitbucket_run.sh 'progs=1 ok=1 diff=0 fail=0 classes=1 pickle_fail=0 known_fail=0 new=0 lint_problems=0'

print -r -- '#!/bin/zsh' > "$FAKE_ROOT/tests/workspace_tests.sh"
print -r -- 'print -r -- "workspace SCALA_RS=${SCALA_RS:-unset} WT_NO_DOC=${WT_NO_DOC:-unset} PICKLE=${PICKLE:-unset} RUNS=${RUNS:-unset}" >> "$ENV_LOG"' >> "$FAKE_ROOT/tests/workspace_tests.sh"
print -r -- 'print -r -- "test result: ok. 1 passed; 0 failed"' >> "$FAKE_ROOT/tests/workspace_tests.sh"
print -r -- 'print -r -- "workspace_tests: binaries=1 rows=8 missing=0 failed_bins=0 doc_rows=7"' >> "$FAKE_ROOT/tests/workspace_tests.sh"
chmod +x "$FAKE_ROOT/tests/workspace_tests.sh"

print -r -- '#!/bin/zsh' > "$FAKE_ROOT/tests/scala_corpus.sh"
print -r -- 'print -r -- "scala_corpus SCALA_RS=${SCALA_RS:-unset} WT_NO_DOC=${WT_NO_DOC:-unset} PICKLE=${PICKLE:-unset} RUNS=${RUNS:-unset}" >> "$ENV_LOG"' >> "$FAKE_ROOT/tests/scala_corpus.sh"
print -r -- 'print -r -- $'"'"'pos\tprobe\tpass'"'"' > "$CORPUS_LOG"' >> "$FAKE_ROOT/tests/scala_corpus.sh"
print -r -- 'print -r -- "pos: total=1 pass=1 fail=0 skip=0"' >> "$FAKE_ROOT/tests/scala_corpus.sh"
chmod +x "$FAKE_ROOT/tests/scala_corpus.sh"

FAKE_RC=0
SCALA_RS=/inherited/compiler WT_NO_DOC=1 PICKLE=0 RUNS=99 KNOWN=/inherited/known \
ROOT="$FAKE_ROOT" GATE_DIR="$FAKE_GATE_DIR" GATE_LEDGER="$WORK/ledger.tsv" \
CORPUS_DIR="$FAKE_CORPUS_DIR" GATE_RESULT_PY="$FAKE_ROOT/tests/gate_result.py" \
ENV_LOG="$ENV_LOG" PATH="$FAKE_BIN_DIR:$PATH" \
  "$FAKE_ROOT/tests/verify_merge.sh" > "$WORK/fake-gate.log" 2>&1 || FAKE_RC=$?
[[ $FAKE_RC == 0 ]] || {
  print -u2 'fake gate failed; see fake-gate.log'
  sed -n '1,220p' "$WORK/fake-gate.log" >&2
  exit 1
}
grep -qx 'VERDICT=PASS' "$WORK/fake-gate.log"
grep -qx 'DONE' "$WORK/fake-gate.log"

for tag in slick_measure cats_measure gitbucket_measure scalalib_measure slick_run slick_subset cats_run gitbucket_run scala_corpus; do
  grep -Eq "^$tag SCALA_RS=$FAKE_GATE_DIR/scala-rs " "$ENV_LOG" || {
    print -u2 "${tag} did not receive the gate binary"
    exit 1
  }
done
grep -Eq '^slick_run .* RUNS=3 KNOWN=/inherited/known$' "$ENV_LOG" || {
  print -u2 'slick_run inherited RUNS'
  exit 1
}
grep -Eq '^cats_run .* PICKLE=1 .* KNOWN=$' "$ENV_LOG" || {
  print -u2 'cats_run inherited PICKLE or KNOWN'
  exit 1
}
grep -Eq '^gitbucket_run .* PICKLE=1 .* KNOWN=$' "$ENV_LOG" || {
  print -u2 'gitbucket_run inherited PICKLE or KNOWN'
  exit 1
}
grep -qx 'workspace SCALA_RS=/inherited/compiler WT_NO_DOC=0 PICKLE=0 RUNS=99' "$ENV_LOG" || {
  print -u2 'workspace did not force WT_NO_DOC=0'
  exit 1
}

print 'verify_merge_test: baseline and inherited-environment contracts passed'
