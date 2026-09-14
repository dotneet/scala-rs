#!/bin/zsh
# Shell adapter for tests/gate_result.py.  Keep protocol validation in Python;
# zsh only deals with process status and the already validated, base64-encoded
# summary field.

GATE_RESULT_PY=${GATE_RESULT_PY:-${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}/tests/gate_result.py}

gate_result_write() {
  local result_path=$1 step_name=$2 result_status=$3 result_rc=$4 result_summary=${5:-}
  python3 "$GATE_RESULT_PY" write "$result_path" "$step_name" "$result_status" "$result_rc" "$result_summary" >/dev/null
}

gate_result_read() {
  local result_path=$1 expected_step=${2:-} row
  if [[ -n $expected_step ]]; then
    row=$(python3 "$GATE_RESULT_PY" read "$result_path" "$expected_step") || return
  else
    row=$(python3 "$GATE_RESULT_PY" read "$result_path") || return
  fi
  local oldifs=$IFS
  IFS=$'\t'
  read -r GATE_RESULT_STEP GATE_RESULT_STATUS GATE_RESULT_RC GATE_RESULT_SUMMARY_B64 <<< "$row"
  IFS=$oldifs
  [[ -n ${GATE_RESULT_STEP:-} && -n ${GATE_RESULT_STATUS:-} && -n ${GATE_RESULT_RC:-} && -n ${GATE_RESULT_SUMMARY_B64+x} ]] || return 2
  if [[ $GATE_RESULT_SUMMARY_B64 == - ]]; then
    GATE_RESULT_SUMMARY=
  else
    GATE_RESULT_SUMMARY=$(printf '%s' "$GATE_RESULT_SUMMARY_B64" | base64 --decode 2>/dev/null) || return
  fi
  return 0
}

# libtest's result grammar has repeated separators.  Use + here so the
# semantic fields are stable: passed is field 4 and failed is field 6.
gate_workspace_counts() {
  local log_file=$1
  awk -F'[ ;]+' '
    /^test result:/ {
      rows++
      if ($4 !~ /^[0-9]+$/ || $5 != "passed" || $6 !~ /^[0-9]+$/ || $7 != "failed") bad=1
      passed += $4; failed += $6
    }
    END {
      if (rows == 0 || bad) exit 2
      print rows "\t" passed "\t" failed
    }' "$log_file"
}

# Extract one numeric key=value metric from a one-line measure summary. Require
# exactly one well-formed occurrence so a truncated or duplicated field cannot
# silently select a plausible number.
gate_measure_field() {
  local summary=$1 key=$2
  awk -v key="$key" '
    {
      for (i = 1; i <= NF; i++) {
        if ($i ~ ("^" key "=[0-9]+$")) { count++; value=$i }
      }
    }
    END {
      if (count != 1) exit 2
      sub(/^[^=]*=/, "", value)
      print value
    }' <<< "$summary"
}
