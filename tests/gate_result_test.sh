#!/bin/zsh
# Shell-level regression checks for the gate-result adapters.
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=$(mktemp -d "${TMPDIR:-/tmp}/scala-rs-gate-result-test.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
source "$ROOT/tests/gate_result.sh"

cat > "$WORK/workspace.log" <<'EOF'
test result: ok. 4 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
EOF
[[ $(gate_workspace_counts "$WORK/workspace.log") == $'2\t7\t2' ]]
if gate_workspace_counts "$WORK/empty.log" >/dev/null 2>&1; then
  print "workspace result helper accepted a log with no result rows" >&2
  exit 1
fi
print 'test result: malformed' > "$WORK/malformed.log"
if gate_workspace_counts "$WORK/malformed.log" >/dev/null 2>&1; then
  print "workspace result helper accepted malformed fields" >&2
  exit 1
fi
[[ $(gate_measure_field 'files=538 errors=383 files_with_errors=107' files) == 538 ]]
[[ $(gate_measure_field 'files=538 errors=383 files_with_errors=107' errors) == 383 ]]
if gate_measure_field 'files=538 files=999' files >/dev/null 2>&1; then
  print "measure helper accepted duplicate fields" >&2
  exit 1
fi
print "gate result shell checks: workspace counts and missing-row rejection passed"
