#!/bin/zsh
# Offline regression checks for slick_subset's fail-closed compiler
# diagnostic handling. This exercises the parser directly so a fake compiler
# never needs the external Slick checkout or a full classfile build.
set -e

ROOT=$(cd "$(dirname "$0")/.." && pwd)
source "$ROOT/tests/slick_subset_errors.sh"
WORK=$(mktemp -d "${TMPDIR:-/tmp}/scala-rs-slick-subset-test.XXXXXX")
trap 'rm -rf -- "$WORK"' EXIT INT TERM

GOOD="$WORK/source tree/has spaces.scala"
mkdir -p "${GOOD:h}"
print -r -- 'error: expected type mismatch' > "$WORK/good.log"
print -r -- "  --> $GOOD:12:7" >> "$WORK/good.log"
GOOD_BAD="$WORK/good.bad"
if ! slick_subset_parse_errors "$WORK/good.log" "$GOOD_BAD"; then
  print -u2 'parseable type-error diagnostics were rejected'
  exit 1
fi
[[ $SLICK_SUBSET_ERROR_COUNT == 1 ]]
[[ $SLICK_SUBSET_LOCATION_COUNT == 1 ]]
[[ $(<"$GOOD_BAD") == "$GOOD" ]]

# A compiler may fail without printing a diagnostic at all (panic, signal,
# launcher failure, and so on). The caller must not interpret that as a
# converged clean set.
print -r -- 'compiler terminated unexpectedly' > "$WORK/no-error.log"
NO_ERROR_RC=0
slick_subset_parse_errors "$WORK/no-error.log" "$WORK/no-error.bad" || NO_ERROR_RC=$?
[[ $NO_ERROR_RC == 1 ]]

# An error line with no parseable source location is also not safe to feed into
# the eviction loop: silently retaining every file would make a nonzero
# compiler exit look like a clean fixpoint.
print -r -- 'error: malformed diagnostic' > "$WORK/malformed.log"
print -r -- '  found: Int' >> "$WORK/malformed.log"
print -r -- '  required: String' >> "$WORK/malformed.log"
MALFORMED_RC=0
slick_subset_parse_errors "$WORK/malformed.log" "$WORK/malformed.bad" || MALFORMED_RC=$?
[[ $MALFORMED_RC == 2 ]]

print -r -- 'slick subset diagnostic parsing: PASS'
