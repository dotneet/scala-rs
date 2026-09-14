#!/bin/zsh
# Small readers for invariants recorded in tests/BASELINE.md.

# Read the one current scalalib row, and only that row. Keep the markdown
# parser strict: stripping non-digits from a malformed cell turns e.g.
# `**2 (old)**` into a plausible but wrong baseline. A duplicated or malformed
# row is not a usable invariant and must fail closed.
read_scalalib_baseline() {
  local baseline_path=${1:-tests/BASELINE.md}
  awk -F'|' '
    $2 ~ /^[[:space:]]*`tests\/scalalib_measure\.sh`([[:space:]]+\([0-9]+\))?[[:space:]]*$/ {
      rows++
      if ($3 !~ /^[[:space:]]*\*\*[0-9]+\*\*[[:space:]]*$/ ||
          $4 !~ /^[[:space:]]*\*\*[0-9]+\*\*[[:space:]]*$/) bad=1
      errors=$3; files=$4
      gsub(/[^0-9]/, "", errors); gsub(/[^0-9]/, "", files)
    }
    END {
      if (rows != 1 || bad || errors !~ /^[0-9]+$/ || files !~ /^[0-9]+$/) exit 2
      print errors "\t" files
    }
  ' "$baseline_path"
}

# Resolve only the corpus ledger named by the current commit row. Historical
# links in BASELINE.md are documentation, not fallback candidates: choosing
# one when the current ledger is missing compares unlike revisions.
read_corpus_baseline() {
  local baseline_path=${1:-tests/BASELINE.md}
  local baseline_dir=${2:-tests/baselines}
  local current
  current=$(grep -m1 -oE '^\| commit \| `[0-9a-f]{8}`' "$baseline_path" 2>/dev/null \
    | grep -oE '[0-9a-f]{8}') || return 2
  local ledger="$baseline_dir/corpus-$current.tsv"
  [[ -s $ledger ]] || return 2
  print -r -- "$ledger"
}
