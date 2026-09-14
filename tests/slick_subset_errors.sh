#!/bin/zsh
# Parse the compiler diagnostics used by slick_subset.sh's type-error
# fixpoint.  A non-empty error set is successful only when every error has a
# source location in the diagnostic shape emitted by scala-rs.  The caller can
# therefore distinguish an expected type-error exit from a compiler crash or
# an output format it cannot safely interpret.

SLICK_SUBSET_ERROR_COUNT=0
SLICK_SUBSET_LOCATION_COUNT=0

slick_subset_parse_errors() {
  local log=$1 output=$2 metadata parse_status=0
  : > "$output"
  metadata=$(python3 - "$log" "$output" <<'PY'
import re
import sys

log_path, output_path = sys.argv[1:]
with open(log_path, encoding="utf-8", errors="replace") as source:
    lines = source.read().splitlines()

location_re = re.compile(r"^\s+--> (.+?\.scala):[0-9]+(?:[: ].*)?$")
errors = 0
locations = []
malformed = 0
for index, line in enumerate(lines):
    if not line.startswith("error"):
        continue
    errors += 1
    matches = [location_re.match(candidate) for candidate in lines[index + 1:index + 3]]
    matches = [match for match in matches if match]
    if len(matches) != 1:
        malformed += 1
    else:
        locations.append(matches[0].group(1))

with open(output_path, "w", encoding="utf-8") as output:
    if errors and not malformed:
        for location in sorted(set(locations)):
            output.write(location + "\n")
print(errors, len(locations), malformed)
sys.exit(1 if errors == 0 else 2 if malformed else 0)
PY
) || parse_status=$?
  read -r SLICK_SUBSET_ERROR_COUNT SLICK_SUBSET_LOCATION_COUNT SLICK_SUBSET_MALFORMED <<< "$metadata"
  # Return 1 for a clean log, 2 for an error block that cannot be mapped to
  # the input source set, and 0 for a fully parseable set of type errors.
  return $parse_status
}
