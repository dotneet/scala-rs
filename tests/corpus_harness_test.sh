#!/usr/bin/env bash
# Exercise corpus and specialization ledger bookkeeping without a compiler
# build or a full corpus run.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=$(mktemp -d /tmp/scala-rs-codex-conformance-audit.XXXXXX)
trap 'rm -rf "$WORK"' EXIT

BIN="$WORK/bin"
CORPUS="$WORK/corpus"
LOG="$WORK/tool.log"
mkdir -p "$BIN" "$CORPUS/test/files/pos"

cat > "$BIN/fake-scala-rs" <<'EOF'
#!/bin/zsh
set -e
log=${FAKE_TOOL_LOG:?}
print -r -- "scala-rs $*" >> "$log"
out=
for ((i = 1; i <= $#; i++)); do
  if [[ ${argv[$i]} == -d ]]; then
    out=${argv[$((i + 1))]}
  fi
done
if [[ -z $out ]]; then
  for arg in "$@"; do
    [[ $arg == -d ]] && continue
  done
  exit 91
fi
mkdir -p "$out"
for arg in "$@"; do
  [[ $arg == *.scala ]] || continue
  : > "$out/${arg:t:r}.class"
done
exit "${FAKE_RS_EXIT:-0}"
EOF

cat > "$BIN/fake-scalac" <<'EOF'
#!/bin/zsh
set -e
log=${FAKE_TOOL_LOG:?}
print -r -- "scalac $*" >> "$log"
out=
for ((i = 1; i <= $#; i++)); do
  if [[ ${argv[$i]} == -d ]]; then
    out=${argv[$((i + 1))]}
  fi
done
[[ -n $out ]] || exit 92
mkdir -p "$out"
for arg in "$@"; do
  [[ $arg == *.scala ]] || continue
  : > "$out/${arg:t:r}.class"
done
exit "${FAKE_SCALAC_EXIT:-0}"
EOF

cat > "$BIN/git" <<'EOF'
#!/bin/zsh
if [[ $1 == -C && $3 == rev-parse && $4 == HEAD ]]; then
  if [[ ${FAKE_GIT_REV:-expected} == expected ]]; then
    print -r -- 3f6bdaeafde17d790023cc3f299b81eaaf876ca3
  else
    print -r -- 0000000000000000000000000000000000000000
  fi
  exit 0
fi
exec "${REAL_GIT:?}" "$@"
EOF

cat > "$BIN/xargs" <<'EOF'
#!/bin/zsh
set -e
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
if [[ ${FAKE_XARGS_MODE:-all} == drop ]]; then
  IFS= read -r first || true
  [[ -n ${first:-} ]] && print -r -- "$first" > "$tmp"
  "${REAL_XARGS:?}" "$@" < "$tmp"
  exit $?
fi
"${REAL_XARGS:?}" "$@"
rc=$?
if [[ ${FAKE_XARGS_MODE:-all} == fail ]]; then
  exit 91
fi
exit $rc
EOF

chmod +x "$BIN"/*
printf 'class A\n' > "$CORPUS/test/files/pos/a.scala"
printf 'class B\n' > "$CORPUS/test/files/pos/b.scala"
printf 'class SpecA\n' > "$CORPUS/test/files/pos/spec-a.scala"
printf 'class SpecB\n' > "$CORPUS/test/files/pos/spec-b.scala"

REAL_GIT=$(command -v git)
REAL_XARGS=$(command -v xargs)
export PATH="$BIN:$PATH" REAL_GIT REAL_XARGS
export SCALA_RS="$BIN/fake-scala-rs" SCALA_RS_PREBUILT=1
export SCALAC="$BIN/fake-scalac"
export CORPUS_DIR="$CORPUS" ROOT
export FAKE_TOOL_LOG="$LOG"

expect_exit() {
  local expected=$1 output=$2
  shift 2
  set +e
  "$@" > "$output" 2>&1
  local actual=$?
  set -e
  if [[ $actual -ne $expected ]]; then
    cat "$output" >&2
    echo "expected exit $expected, got $actual: $*" >&2
    exit 1
  fi
}

run_corpus() {
  local mode=$1 log="$WORK/corpus-$1.tsv" output="$WORK/corpus-$1.out"
  rm -f "$log" "$output" "$LOG"
  : > "$LOG"
  CORPUS_KINDS=pos CORPUS_SIZE=full CORPUS_JOBS=1 CORPUS_LOG="$log" \
    CORPUS_WORK="$WORK/work-corpus-$mode" CORPUS_NO_REPORT=1 \
    FAKE_GIT_REV=expected FAKE_XARGS_MODE="$mode" \
    tests/scala_corpus.sh > "$output" 2>&1
}

run_spec() {
  local mode=$1 log="$WORK/spec-$1.tsv" output="$WORK/spec-$1.out"
  rm -f "$log" "$output" "$LOG"
  : > "$LOG"
  SPEC_JOBS=1 SPEC_LOG="$log" SPEC_WORK="$WORK/work-spec-$mode" \
    FAKE_GIT_REV=expected FAKE_XARGS_MODE="$mode" \
    tests/spec_classfiles.sh > "$output" 2>&1
}

run_corpus all
[[ $(wc -l < "$WORK/corpus-all.tsv" | tr -d ' ') == 4 ]]
grep -q 'pos.*pass' "$WORK/corpus-all.tsv"

expect_exit 2 "$WORK/corpus-drop.out" env \
  CORPUS_KINDS=pos CORPUS_SIZE=full CORPUS_JOBS=1 \
  CORPUS_LOG="$WORK/corpus-drop.tsv" CORPUS_WORK="$WORK/work-corpus-drop" \
  CORPUS_NO_REPORT=1 FAKE_GIT_REV=expected FAKE_XARGS_MODE=drop \
  FAKE_TOOL_LOG="$LOG" SCALA_RS="$BIN/fake-scala-rs" SCALA_RS_PREBUILT=1 \
  CORPUS_DIR="$CORPUS" ROOT="$ROOT" SCALAC="$BIN/fake-scalac" \
  PATH="$BIN:$PATH" tests/scala_corpus.sh
grep -q 'corpus harness incomplete: expected_rows=4 actual_rows=1' "$WORK/corpus-drop.out"

expect_exit 2 "$WORK/corpus-revision.out" env \
  CORPUS_KINDS=pos CORPUS_SIZE=full CORPUS_JOBS=1 \
  CORPUS_LOG="$WORK/corpus-revision.tsv" CORPUS_WORK="$WORK/work-corpus-revision" \
  CORPUS_NO_REPORT=1 FAKE_GIT_REV=wrong FAKE_XARGS_MODE=all \
  FAKE_TOOL_LOG="$LOG" SCALA_RS="$BIN/fake-scala-rs" SCALA_RS_PREBUILT=1 \
  CORPUS_DIR="$CORPUS" ROOT="$ROOT" PATH="$BIN:$PATH" tests/scala_corpus.sh
grep -q 'corpus revision mismatch' "$WORK/corpus-revision.out"

expect_exit 2 "$WORK/spec-drop.out" env \
  SPEC_JOBS=1 SPEC_LOG="$WORK/spec-drop.tsv" SPEC_WORK="$WORK/work-spec-drop" \
  FAKE_GIT_REV=expected FAKE_XARGS_MODE=drop FAKE_TOOL_LOG="$LOG" \
  SCALA_RS="$BIN/fake-scala-rs" SCALA_RS_PREBUILT=1 SCALAC="$BIN/fake-scalac" \
  CORPUS_DIR="$CORPUS" ROOT="$ROOT" PATH="$BIN:$PATH" tests/spec_classfiles.sh
grep -q 'specialization ledger incomplete: expected_rows=2 actual_rows=1' "$WORK/spec-drop.out"

expect_exit 2 "$WORK/spec-xargs.out" env \
  SPEC_JOBS=1 SPEC_LOG="$WORK/spec-xargs.tsv" SPEC_WORK="$WORK/work-spec-xargs" \
  FAKE_GIT_REV=expected FAKE_XARGS_MODE=fail FAKE_TOOL_LOG="$LOG" \
  SCALA_RS="$BIN/fake-scala-rs" SCALA_RS_PREBUILT=1 SCALAC="$BIN/fake-scalac" \
  CORPUS_DIR="$CORPUS" ROOT="$ROOT" PATH="$BIN:$PATH" tests/spec_classfiles.sh
grep -q 'worker_exit=91' "$WORK/spec-xargs.out"

expect_exit 2 "$WORK/spec-revision.out" env \
  SPEC_JOBS=1 SPEC_LOG="$WORK/spec-revision.tsv" SPEC_WORK="$WORK/work-spec-revision" \
  FAKE_GIT_REV=wrong FAKE_XARGS_MODE=all FAKE_TOOL_LOG="$LOG" \
  SCALA_RS="$BIN/fake-scala-rs" SCALA_RS_PREBUILT=1 SCALAC="$BIN/fake-scalac" \
  CORPUS_DIR="$CORPUS" ROOT="$ROOT" PATH="$BIN:$PATH" tests/spec_classfiles.sh
grep -q 'corpus revision mismatch' "$WORK/spec-revision.out"

echo 'corpus harness: complete, missing-row, worker-error, and revision gates passed'
