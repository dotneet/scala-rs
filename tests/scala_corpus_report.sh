#!/bin/zsh
# Summarise a tests/scala_corpus.sh log: pass rates per category, and the
# symptoms behind the failures, most frequent first.
#
# Usage: tests/scala_corpus_report.sh <corpus.tsv> [top-N]
#
# The log is `kind<TAB>name<TAB>status<TAB>symptom`, with the symptom kept
# verbatim from the compiler. Bucketing happens here rather than in the runner
# so that a log recorded once can be re-cut a different way.
set -e
LOG=${1:?usage: scala_corpus_report.sh <corpus.tsv> [top-N]}
TOP=${2:-15}

# Fold a diagnostic into a bucket: drop the `error:` prefix, replace anything
# in backticks/quotes with X, and cut the "found/required" tail, so that the
# same missing feature lands in one row instead of two hundred.
norm() {
  perl -pe '
    s/^error(\[[^\]]*\])?:\s*//;
    s/[`\x27"][^`\x27"]*[`\x27"]/X/g;
    s/;\s*found:.*//; s/\s+required:.*//;
    s/\bat [\/\w.\-]+:\d+.*//;
    s/[ \t]+/ /g; s/[ \t]+$//;
  ' | cut -c1-72
}

for kind in pos neg run; do
  tot=$(awk -F'\t' -v k=$kind '$1==k' $LOG | wc -l | tr -d ' ')
  (( tot == 0 )) && continue
  p=$(awk -F'\t' -v k=$kind '$1==k && $3=="pass"' $LOG | wc -l | tr -d ' ')
  f=$(awk -F'\t' -v k=$kind '$1==k && $3=="fail"' $LOG | wc -l | tr -d ' ')
  s=$(awk -F'\t' -v k=$kind '$1==k && $3=="skip"' $LOG | wc -l | tr -d ' ')
  rate=0
  (( tot - s > 0 )) && rate=$(( 100.0 * p / (tot - s) ))
  echo
  printf '=== %s: total=%d pass=%d fail=%d skip=%d pass_rate=%.1f%% (of non-skipped)\n' \
    $kind $tot $p $f $s $rate
  echo "--- top $TOP failure symptoms"
  awk -F'\t' -v k=$kind '$1==k && $3=="fail" {print $4}' $LOG \
    | norm | sort | uniq -c | sort -rn | head -$TOP
  echo "--- skip reasons"
  awk -F'\t' -v k=$kind '$1==k && $3=="skip" {print $4}' $LOG \
    | sed -e 's/^\(unsupported-flag\) .*/\1/' -e 's/^\(crash\) .*/\1/' \
    | sort | uniq -c | sort -rn | head -$TOP
done

echo
echo "=== neg passes, by which diagnostic did the rejecting"
awk -F'\t' '$1=="neg" && $3=="pass" {print $4}' $LOG \
  | norm | sort | uniq -c | sort -rn | head -$TOP
