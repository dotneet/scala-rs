#!/bin/zsh
# Run workspace_tests against a tiny fake Cargo/test-binary graph. The
# executable and package paths intentionally contain spaces; this catches a
# regression to xargs' default whitespace parser without compiling the real
# workspace.
set -e

ROOT=$(cd "$(dirname "$0")/.." && pwd)
BASE=$(mktemp -d "${TMPDIR:-/tmp}/scala-rs-workspace-test.XXXXXX")
WORK="$BASE/repo path"
FAKE_BIN="$WORK/fake bin"
PKG_ONE="$WORK/package one"
PKG_TWO="$WORK/package two"
RUN_LOG="$WORK/run log"
OUT_LOG="$WORK/workspace output"
trap 'rm -rf -- "$BASE"' EXIT INT TERM
mkdir -p "$FAKE_BIN" "$PKG_ONE" "$PKG_TWO"

make_test_binary() {
  local file_path=$1 count=$2
  print -r -- '#!/bin/zsh' > "$file_path"
  print -r -- 'if [[ $1 == --list ]]; then' >> "$file_path"
  for (( i = 1; i <= count; i++ )); do
    print -r -- "  print -r -- 'test_$i: test'" >> "$file_path"
  done
  print -r -- '  exit 0' >> "$file_path"
  print -r -- 'fi' >> "$file_path"
  print -r -- 'print -r -- "$0|$PWD" >> "$RUN_LOG"' >> "$file_path"
  print -r -- "print -r -- 'test result: ok. $count passed; 0 failed'" >> "$file_path"
  chmod +x "$file_path"
}

ONE="$FAKE_BIN/slow test"
TWO="$FAKE_BIN/fast test"
make_test_binary "$ONE" 3
make_test_binary "$TWO" 1

BUILD_JSON="$WORK/build json"
python3 - "$BUILD_JSON" "$ONE" "$TWO" "$PKG_ONE" "$PKG_TWO" <<'PY'
import json, sys
out, one, two, pkg_one, pkg_two = sys.argv[1:]
records = [
    {"reason": "compiler-artifact", "executable": one,
     "profile": {"test": True}, "manifest_path": pkg_one + "/Cargo.toml"},
    {"reason": "compiler-artifact", "executable": two,
     "profile": {"test": True}, "manifest_path": pkg_two + "/Cargo.toml"},
]
with open(out, "w") as f:
    for record in records:
        f.write(json.dumps(record) + "\n")
PY

CARGO="$FAKE_BIN/cargo"
print -r -- '#!/bin/zsh' > "$CARGO"
print -r -- 'if [[ $* == *"--no-run"* ]]; then cat "$BUILD_JSON"; exit 0; fi' >> "$CARGO"
print -r -- 'if [[ $* == *"--doc"* ]]; then print -r -- "test result: ok. 1 passed; 0 failed"; exit 0; fi' >> "$CARGO"
print -r -- 'print -u2 "unexpected fake cargo invocation"' >> "$CARGO"
print -r -- 'exit 90' >> "$CARGO"
chmod +x "$CARGO"

: > "$RUN_LOG"
PATH="$FAKE_BIN:$PATH" BUILD_JSON="$BUILD_JSON" RUN_LOG="$RUN_LOG" \
  ROOT="$ROOT" WT_DIR="$WORK/work dir" WT_JOBS=1 WT_THREADS=1 WT_TIMEOUT=5 \
  "$ROOT/tests/workspace_tests.sh" > "$OUT_LOG" 2>&1

grep -qx 'workspace_tests: binaries=2 rows=3 missing=0 failed_bins=0 doc_rows=1' "$OUT_LOG"
[[ $(wc -l < "$RUN_LOG" | tr -d ' ') == 2 ]]
grep -F -- "$ONE|" "$RUN_LOG" >/dev/null
grep -F -- "$TWO|" "$RUN_LOG" >/dev/null
grep -F -- 'package one' "$RUN_LOG" >/dev/null
grep -F -- 'package two' "$RUN_LOG" >/dev/null

print -r -- 'workspace test path handling: PASS'
