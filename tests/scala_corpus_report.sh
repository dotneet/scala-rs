#!/bin/zsh
# Summarise a tests/scala_corpus.sh log: pass rates per category, and the
# symptoms behind the failures, most frequent first.
#
# Usage: tests/scala_corpus_report.sh <corpus.tsv> [top-N]
#
# The log is `kind<TAB>name<TAB>status<TAB>symptom`, with the symptom kept
# verbatim from the compiler. Bucketing happens here rather than in the runner
# so that a log recorded once can be re-cut a different way.
# Reference diagnostics can contain arbitrary bytes. This process only
# reports saved results; it does not change a compiler or JVM locale.
export LC_ALL=C
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

# The `neg` failures are the serious ones: a program scalac rejects that we
# accept. Bucket them by what scalac itself says in the `.check`, which is the
# closest thing to a list of the checks we do not perform.
CORPUS=${CORPUS_DIR:-/tmp/scala-rs-corpus/scala}
if [[ -d $CORPUS/test/files/neg ]]; then
  echo
  echo "=== neg failures (we accept it), by the diagnostic scalac expects"
  for n in $(awk -F'\t' '$1=="neg" && $3=="fail" {print $2}' $LOG); do
    c=$CORPUS/test/files/neg/$n.check
    [[ -f $c ]] || { echo "(no .check)"; continue; }
    # Prefer a real error; some `.check` files open with warnings that only
    # become errors under a `-Werror` the test also asks for.
    grep -m1 -E '^\S+\.scala:[0-9]+: error:' $c 2>/dev/null \
      || grep -m1 -E '^\S+\.scala:[0-9]+: warning:' $c 2>/dev/null \
      || grep -m1 -E '^error:|^warning:' $c 2>/dev/null \
      || echo "(no diagnostic line in .check)"
  done | perl -pe '
      s/^\S+\.scala:\d+:\s*//;
      s/^(error|warning)(:|\s)\s*//;
      s/[`\x27"][^`\x27"]*[`\x27"]/X/g;
      s/;\s*found:.*//; s/\s+required:.*//;
      s/[ \t]+/ /g; s/[ \t]+$//;
    ' | cut -c1-72 | sort | uniq -c | sort -rn | head -$(( TOP * 2 ))
fi

# ---------------------------------------------------------------------------
# `neg`, scored against the `.check` text.
#
# Column 3 ("any error is a pass") is an upper bound: it counts a rejection for
# the wrong reason. Columns 5 and 6 of the log hold our diagnostics and the
# ones scalac's `.check` records, both as `<file>:<line>: <level>: <message>`
# records joined by \x1e, so the two can be compared here.
#
# A full-text comparison is not available and never will be: scalac prints
# `type mismatch;` with its found/required on continuation lines and renders a
# constant type as `String("Hello")` where we render `"Hello"`, so comparing
# the tail would measure the type printer rather than the type checker. What is
# comparable is the *head* of the message -- everything before the first `;`
# and before the end of the first sentence, case- and whitespace-folded. That
# is what "same diagnostic" means below.
#
# Three tiers are printed, none of which replaces column 3:
#   T1  every diagnostic the .check expects has a match somewhere (multiset:
#       four expected copies need four of ours), ignoring where it was reported
#   T2  ... and each match is at the file and line scalac reports it at
#   T3  ... and we emit no diagnostics beyond the expected count
# ---------------------------------------------------------------------------
echo
perl -e '
  sub norm {
    my ($m) = @_;
    $m = lc $m;
    $m =~ s/\s+/ /g; $m =~ s/^ //; $m =~ s/ $//;
    $m =~ s/;.*$//;        # scalac carries found/required past the ";"
    $m =~ s/\.\s.*$//;     # ... and the rest of an explanation past the "."
    $m =~ s/[.\s]+$//;
    return $m;
  }
  sub rec {
    my ($r) = @_;
    my ($f, $l, $lv, $m) = $r =~ /^(\S+?):(\d+): (error|warning): (.*)$/ or return ();
    return ($f, $l, norm($m));
  }
  my $top = shift @ARGV;
  my ($tot, $any, $t1, $t2, $t3, $nowant) = (0) x 6;
  my (%buck, %pairs, %amsg, %omsg);
  open(my $fh, "<", $ARGV[0]) or exit 0;
  while (<$fh>) {
    chomp; my @f = split /\t/, $_, -1;
    next unless $f[0] eq "neg"; next if $f[2] eq "skip";
    $tot++; $any++ if $f[2] eq "pass";
    my (%gm, %gl, @gn); my $ng = 0;
    for my $g (grep { length } split /\x1e/, ($f[4] // "")) {
      my ($ff, $l, $n) = rec($g); next unless defined $n;
      $ng++; $gm{$n}++; $gl{"$ff:$l\x00$n"}++; push @gn, $n;
    }
    my (@wn, @wl); my $nw = 0;
    for my $w (grep { length } split /\x1e/, ($f[5] // "")) {
      my ($ff, $l, $n) = rec($w); next unless defined $n;
      $nw++; push @wn, $n; push @wl, "$ff:$l\x00$n";
    }
    if (!$nw) { $nowant++; next }
    my %a = %gm; my $ok = 0;
    for my $n (@wn) { if (($a{$n} // 0) > 0) { $a{$n}--; $ok++ } }
    my %b = %gl; my $okl = 0;
    for my $k (@wl) { if (($b{$k} // 0) > 0) { $b{$k}--; $okl++ } }
    my $extra = $ng > $nw;
    $t1++ if $ok == $nw;
    $t2++ if $okl == $nw;
    $t3++ if $okl == $nw && !$extra;
    my $bk = !$ng     ? "a we accept the program, no diagnostic at all"
           : $ok == 0 ? "b we reject it, but for none of the expected reasons"
           : $ok < $nw  ? "c partial: some of the expected diagnostics reproduced"
           : $okl < $nw ? "d right messages, wrong file or line"
           : $extra     ? "e right messages and lines, plus extra of our own"
           :              "f exact match";
    $buck{$bk}++;
    $amsg{$wn[0]}++ if $bk =~ /^a/;
    if ($bk =~ /^[bc]/) {
      $omsg{$gn[0]}++;
      $pairs{ sprintf("%-52.52s <= %.52s", $wn[0], $gn[0]) }++;
    }
  }
  exit 0 unless $tot;
  printf "=== neg, scored against the .check text (%d non-skipped)\n", $tot;
  printf "  T0 any error at all (column 3)          %5d  %5.1f%%   upper bound\n", $any, 100*$any/$tot;
  printf "  T1 expected messages reproduced         %5d  %5.1f%%\n", $t1, 100*$t1/$tot;
  printf "  T2 ... at the expected file and line    %5d  %5.1f%%\n", $t2, 100*$t2/$tot;
  printf "  T3 ... and nothing extra                %5d  %5.1f%%\n", $t3, 100*$t3/$tot;
  printf "  (%d tests have no error or warning line in their .check and are\n", $nowant if $nowant;
  printf "   left out of T1-T3; they are still in the %d and in column 3.)\n", $tot if $nowant;
  print "--- how the wording differs\n";
  printf "  %5d  %s\n", $buck{$_}, $_ for sort keys %buck;
  print "--- a: what scalac says about the programs we accept\n";
  my @a = sort { $amsg{$b} <=> $amsg{$a} } keys %amsg;
  printf("  %4d  %.72s\n", $amsg{$_}, $_) for @a[0 .. ($#a < $top-1 ? $#a : $top-1)];
  print "--- b+c: the diagnostic we gave instead\n";
  my @o = sort { $omsg{$b} <=> $omsg{$a} } keys %omsg;
  printf("  %4d  %.72s\n", $omsg{$_}, $_) for @o[0 .. ($#o < $top-1 ? $#o : $top-1)];
  print "--- b+c: expected <= ours, paired\n";
  my @p = sort { $pairs{$b} <=> $pairs{$a} } keys %pairs;
  printf("  %4d  %s\n", $pairs{$_}, $_) for @p[0 .. ($#p < $top-1 ? $#p : $top-1)];
' $TOP $LOG
