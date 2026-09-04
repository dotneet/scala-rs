#!/bin/zsh
# Run scala/scala's own test corpus (test/files/{pos,neg,run}) against scala-rs
# and report how much of it we accept, reject and reproduce.
#
# Modelled on tests/slick_measure.sh and tests/cats_measure.sh; the same rules
# apply:
#   * pin the revision and re-fetch the material when it is missing, so a
#     reboot that wipes /tmp costs one slow run and not a debugging session;
#   * every path this script writes is per-invocation, so two runs at once
#     cannot report each other's numbers;
#   * point CORPUS_LOG at a path of your own -- the default is shared.
#
# This is NOT partest. partest needs sbt and a built compiler; we read the
# `.scala` and `.check` files directly, which is both faster and something we
# can reason about.
#
#   pos/   pass when scala-rs compiles the sources with zero errors
#   neg/   pass when scala-rs reports at least one error. That is an upper
#          bound -- a rejection for the wrong reason counts -- so the log also
#          carries our diagnostics and the ones the `.check` expects, and
#          tests/scala_corpus_report.sh scores the wording on top of it. Both
#          numbers are reported; neither replaces the other.
#   run/   pass when it compiles, `java Test` runs, and stdout matches `.check`
#
# scala-rs is a subset implementation, so most of the corpus is expected to
# fail. The number is the product.
#
# Usage:
#   CORPUS_LOG=$MYDIR/corpus.tsv tests/scala_corpus.sh
#
# Environment:
#   CORPUS_LOG      result TSV (default: shared scratchpad path -- override it)
#   CORPUS_KINDS    space-separated subset of "pos neg run" (default: all three)
#   CORPUS_SIZE     "sample" (default) or "full".  `sample` takes an evenly
#                   spaced, deterministic CORPUS_SAMPLE tests per category, so
#                   a run costs minutes instead of an hour.
#   CORPUS_SAMPLE   sample size per category (default 250)
#   CORPUS_FILTER   only run tests whose path matches this zsh glob pattern,
#                   e.g. CORPUS_FILTER='(t2973|u000a)'
#   CORPUS_JOBS     parallel workers (default 8)
#   CORPUS_TIMEOUT  seconds per compile (default 40)
#   CORPUS_RUN_TIMEOUT seconds per `java Test` (default 20)
#   SCALA_RS        use this binary instead of building target/release/scala-rs
set -e

SP=/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/scalacorpus
# v2.13.16 -- the same release as the real scalac the conform suite dual-runs
# against, so a disagreement is about us and not about a version skew.
SCALA_REV=3f6bdaeafde17d790023cc3f299b81eaaf876ca3
CORPUS=${CORPUS_DIR:-/tmp/scala-rs-corpus/scala}
LIB=/tmp/scala-rs-lib/scala-library-2.13.16.jar
# partest compiles and runs against scala-reflect and scala-compiler as well;
# without them every `scala.reflect.runtime.universe` test fails for a reason
# that has nothing to do with our type checker.
EXTRA_CP=/tmp/scala-2.13.16/lib/scala-reflect.jar:/tmp/scala-2.13.16/lib/scala-compiler.jar
CCACHE=$HOME/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2

# ---------------------------------------------------------------------------
# Worker mode: `$0 --one <kind>:<path>` runs a single test and appends one TSV
# line to $CORPUS_LOG. Everything it needs comes from the environment, which
# xargs passes through.
# ---------------------------------------------------------------------------
if [[ $1 == --one ]]; then
  spec=$2
  kind=${spec%%:*}
  tpath=${spec#*:}
  name=${tpath:t:r}
  BIN=$SCALA_RS
  TMO=${CORPUS_TIMEOUT:-40}
  RTMO=${CORPUS_RUN_TIMEOUT:-20}
  WORK=$CORPUS_WORK/$kind/$name
  rm -rf $WORK; mkdir -p $WORK/out

  emit() {  # status symptom [got] [want]
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
      $kind $name $1 "$2" "${3:-}" "${4:-}" >> $CORPUS_LOG.part/$kind-$name
  }

  # --- diagnostic capture, for the `neg` wording comparison ------------------
  # Both sides are reduced to a list of scalac-shaped records
  #     <basename>:<line>: <level>: <message>
  # joined by an ASCII record separator (\x1e), which cannot occur in either
  # compiler's output and keeps one test on one TSV line. Only the *first* line
  # of a message is kept: ours are one line each, while scalac puts the
  # found/required of a `type mismatch;` on continuation lines, so the heads are
  # the only thing the two renderings have in common. Scoring is deliberately
  # left to tests/scala_corpus_report.sh so that a log recorded once can be
  # re-cut under a different notion of "same message".

  # Our own diagnostics, read back out of the compile logs. Warnings are kept
  # as well as errors: a `neg` test whose `.check` holds only warnings is one
  # that fails under `-Xfatal-warnings`, and there the two compilers can
  # legitimately disagree about the level while agreeing about the message.
  our_diags() {
    perl -ne '
      BEGIN { @o = (); }
      sub flush {
        return unless defined $msg;
        push @o, sprintf("%s:%s: %s: %s", defined $f ? $f : "?",
                         defined $l ? $l : 0, $lvl, substr($msg, 0, 120));
        undef $msg; undef $f; undef $l;
      }
      if (/^(error|warning)(?:\[[^\]]*\])?:\s*(.*)$/) {
        flush(); $lvl = $1; $msg = $2; next;
      }
      if (defined $msg && !defined $l && m{^\s*-->\s*(\S+)}) {
        my $p = $1;
        if ($p =~ /^(.*):(\d+):\d+$/) { $f = $1; $l = $2; } else { $f = $p; $l = 0; }
        $f =~ s{.*/}{};
      }
      END { flush(); print join("\x1e", @o); }
    ' "$@" 2>/dev/null | tr '\t' ' '
  }

  # What the `.check` says scalac reports. The error lines are what a `neg`
  # test is about; the warning lines are taken only when there is no error
  # line at all, which is the shape of a test that fails purely because a
  # warning was promoted. Taking warnings *alongside* errors would score us on
  # lints nobody claims we implement.
  check_diags() {
    [[ -f $1 ]] || return 0
    perl -ne '
      BEGIN { @e = (); @w = (); }
      if (/^(\S+):(\d+):\s*(error|warning):\s*(.*)$/) {
        my ($f, $l, $lvl, $m) = ($1, $2, $3, $4);
        $f =~ s{.*/}{};
        if ($f =~ /\.(scala|java)$/) {
          my $r = sprintf("%s:%s: %s: %s", $f, $l, $lvl, substr($m, 0, 120));
          if ($lvl eq "error") { push @e, $r } else { push @w, $r }
        }
      }
      END { print join("\x1e", @e ? @e : @w); }
    ' $1 2>/dev/null | tr '\t' ' '
  }

  # --- collect the sources -------------------------------------------------
  if [[ -d $tpath ]]; then
    srcs=($tpath/*.scala(N))
    javas=($tpath/*.java(N))
  else
    srcs=($tpath)
    javas=(${tpath:r}.java(N))
  fi
  if (( ${#srcs} == 0 )); then emit skip no-scala-sources; exit 0; fi
  if (( ${#javas} > 0 )); then emit skip java-sources; exit 0; fi

  # partest puts its own extras and junit on the classpath and drives some
  # tests by re-invoking the compiler (`scala.tools.partest.DirectTest` and
  # friends). Those test the compiler's own plumbing, not the language, and
  # we have neither jar, so they are not ours to judge.
  if grep -q -E 'scala\.tools\.partest|org\.junit|scala\.tools\.nsc' $srcs; then
    emit skip needs-partest-or-junit; exit 0
  fi

  # --- compiler options the test asks for ----------------------------------
  # 2.13.16 carries them as a `//> using options ...` header inside the source
  # (the older `.flags` sidecar files are gone -- there is not one left in the
  # tree). A handful of tests still use the intermediate `//scalac: ...` form.
  want=("${(@f)$(grep -h -E '^[[:space:]]*(//> using options|//scalac:|/\* scalac:)' $srcs 2>/dev/null || true)}")
  opts=()
  for line in $want; do
    line=${line#*options }
    line=${line#*scalac:}
    line=${line%\*/}
    for o in ${(z)line}; do
      case $o in
        # Understood, and meaning-preserving.
        -Xsource:3|-Xsource:3.0|-Xsource:3.4) opts+=(-Xsource:3) ;;
        -Xsource:3-cross) opts+=(-Xsource:3-cross) ;;
        -Xfatal-warnings|-Xasync) opts+=($o) ;;
        -language:*|-Xsource-features:*) opts+=($o) ;;
        # Accepted by scalac, no effect on whether we accept the program.
        -deprecation|-unchecked|-feature|-nowarn|-usejavacp|-explaintypes) ;;
        # Anything else changes what scalac accepts or how it reports, so the
        # test is not ours to judge.
        *) emit skip "unsupported-flag $o"; exit 0 ;;
      esac
    done
  done

  # --- compile, in `_N` rounds when the test is a separate-compilation one --
  rounds=(1)
  if [[ ${(j: :)srcs} == *_[0-9].scala* ]]; then
    rounds=(${(onu)${(M)${srcs:t:r}%_[0-9]}##*_})
  fi
  errors=0; crashed=; timedout=; symptom=
  cp_extra=
  for r in $rounds; do
    if (( ${#rounds} > 1 )); then
      group=(${(M)srcs:#*_$r.scala})
      (( ${#group} == 0 )) && continue
    else
      group=($srcs)
    fi
    log=$WORK/round$r.log
    set +e
    perl -e 'alarm shift @ARGV; exec @ARGV' $TMO \
      $BIN compile $group -d $WORK/out -cp "$WORK/out$cp_extra:$EXTRA_CP" \
      --scala-library $LIB $opts > $log 2>&1
    rc=$?
    set -e
    cp_extra=":$WORK/out"
    if (( rc == 142 )); then timedout=1; break; fi
    if grep -q 'panicked at\|stack overflow\|fatal runtime' $log; then crashed=1; break; fi
    if (( rc != 0 && rc != 1 )); then crashed=1; break; fi
    n=$(grep -c '^error' $log || true)
    errors=$(( errors + n ))
    if (( n > 0 )); then
      # Keep the first diagnostic verbatim (minus tabs, which are the field
      # separator). Bucketing happens at report time, so the log stays usable
      # for anything we did not think to bucket by.
      symptom=$(grep -m1 '^error' $log | tr '\t' ' ' | cut -c1-140)
      break
    fi
  done

  if [[ -n $timedout ]]; then emit skip timeout; exit 0; fi
  if [[ -n $crashed ]]; then
    why=$(grep -m1 -o 'panicked at.*\|stack overflow' $log | cut -c1-90)
    emit skip "crash ${why:-exit$rc}"; exit 0
  fi

  case $kind in
    pos)
      # `errors=0` on its own is not proof of a compile: a compiler that fell
      # over quietly also reports no errors. Insist on a classfile, the same
      # second reading the slick and gitbucket measurements use -- but only
      # when the sources define something. A handful of pos tests are a bare
      # `package foo` or nothing but comments, and scalac emits nothing for
      # those either.
      classes=($WORK/out/**/*.class(N))
      if (( errors > 0 )); then emit fail "$symptom"
      elif (( ${#classes} == 0 )) \
        && grep -q -E '^[^/*]*\b(class|trait|object|package object)\b' $srcs; then
        emit fail "compiled but emitted no classfiles"
      else emit pass -; fi ;;
    neg)
      # Column 3 keeps the historical rule -- any error at all is a pass. It is
      # an upper bound and it is still worth having: "we reject this program"
      # is a weaker claim than "we reject it for the right reason", not a
      # meaningless one. Columns 5 and 6 carry the two diagnostic lists so the
      # report can score the wording as well, without a second corpus run.
      ncheck=${tpath:r}.check
      [[ -d $tpath ]] && ncheck=$tpath.check
      want=$(check_diags $ncheck)
      got=$(our_diags $WORK/round*.log(N))
      if (( errors > 0 )); then
        emit pass "$symptom" "$got" "$want"
      else
        emit fail accepted-but-should-not-compile "$got" "$want"
      fi ;;
    run)
      if (( errors > 0 )); then emit fail "$symptom"; exit 0; fi
      check=${tpath:r}.check
      [[ -d $tpath ]] && check=$tpath.check
      set +e
      perl -e 'alarm shift @ARGV; exec @ARGV' $RTMO \
        java -cp "$WORK/out:$LIB:$EXTRA_CP" Test > $WORK/stdout.txt 2> $WORK/stderr.txt
      rc=$?
      set -e
      if (( rc == 142 )); then emit skip run-timeout; exit 0; fi
      if (( rc != 0 )); then
        emit fail "runtime $(head -c 70 $WORK/stderr.txt | head -1)"; exit 0
      fi
      if [[ -f $check ]]; then
        # partest merges stdout and stderr into one log before comparing, so
        # accept either stdout alone or the two concatenated.
        cat $WORK/stdout.txt $WORK/stderr.txt > $WORK/both.txt
        if diff -q $check $WORK/stdout.txt >/dev/null 2>&1 \
           || diff -q $check $WORK/both.txt >/dev/null 2>&1; then
          emit pass -
        else
          emit fail output-mismatch
        fi
      elif [[ ! -s $WORK/stdout.txt ]]; then
        emit pass -
      else
        emit fail unexpected-output
      fi ;;
  esac
  rm -rf $WORK
  exit 0
fi

# ---------------------------------------------------------------------------
# Driver
# ---------------------------------------------------------------------------
if [[ ! -f $LIB ]]; then
  mkdir -p /tmp/scala-rs-lib
  cp $CCACHE/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar /tmp/scala-rs-lib/
fi
if [[ ! -f /tmp/scala-2.13.16/lib/scala-reflect.jar ]]; then
  mkdir -p /tmp/scala-2.13.16/lib
  cp $CCACHE/org/scala-lang/scala-reflect/2.13.16/scala-reflect-2.13.16.jar /tmp/scala-2.13.16/lib/scala-reflect.jar
  cp $CCACHE/org/scala-lang/scala-compiler/2.13.16/scala-compiler-2.13.16.jar /tmp/scala-2.13.16/lib/scala-compiler.jar
fi
if [[ ! -d $CORPUS/test/files ]]; then
  mkdir -p ${CORPUS:h}; rm -rf $CORPUS
  git clone --depth 1 --branch v2.13.16 --filter=blob:none --no-tags \
      https://github.com/scala/scala.git $CORPUS >/dev/null 2>&1
fi
have=$(git -C $CORPUS rev-parse HEAD)
if [[ $have != $SCALA_REV ]]; then
  echo "warning: corpus at $have, expected $SCALA_REV (v2.13.16)" >&2
fi

ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
export SCALA_RS=${SCALA_RS:-$ROOT/target/release/scala-rs}
if [[ ! -x $SCALA_RS ]] || [[ -z ${SCALA_RS_PREBUILT:-} ]]; then
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>/tmp/scala_corpus_build.log \
    || { cat /tmp/scala_corpus_build.log; exit 1; }
fi

export CORPUS_LOG=${CORPUS_LOG:-$SP/corpus.tsv}
export CORPUS_WORK=${CORPUS_WORK:-$SP/work-$$}
mkdir -p ${CORPUS_LOG:h} $CORPUS_WORK
rm -rf $CORPUS_LOG.part; mkdir -p $CORPUS_LOG.part
: > $CORPUS_LOG

KINDS=(${=CORPUS_KINDS:-pos neg run})
SIZE=${CORPUS_SIZE:-sample}
SAMPLE=${CORPUS_SAMPLE:-250}
JOBS=${CORPUS_JOBS:-8}

specs=()
for kind in $KINDS; do
  mkdir -p $CORPUS_LOG.part
  tests=($CORPUS/test/files/$kind/*.scala(N) $CORPUS/test/files/$kind/*(N/))
  tests=(${(o)tests})
  if [[ -n ${CORPUS_FILTER:-} ]]; then
    tests=(${(M)tests:#*${~CORPUS_FILTER}*})
  fi
  if [[ $SIZE == sample ]] && (( ${#tests} > SAMPLE )); then
    # Deterministic even spacing over the alphabetical order: the sample keeps
    # the same tests between runs, so two measurements are comparable, and it
    # is not biased towards one prefix the way `head -n` would be.
    step=$(( ${#tests} / SAMPLE ))
    picked=()
    for ((i = 1; i <= ${#tests} && ${#picked} < SAMPLE; i += step)); do
      picked+=($tests[i])
    done
    tests=($picked)
  fi
  echo "$kind: ${#tests} tests" >&2
  for t in $tests; do specs+=("$kind:$t"); done
done

print -l $specs | xargs -P $JOBS -n 1 -I{} $0 --one {} || true
cat $CORPUS_LOG.part/*(N) | sort > $CORPUS_LOG
rm -rf $CORPUS_LOG.part $CORPUS_WORK

for kind in $KINDS; do
  tot=$(awk -F'\t' -v k=$kind '$1==k' $CORPUS_LOG | wc -l | tr -d ' ')
  p=$(awk -F'\t' -v k=$kind '$1==k && $3=="pass"' $CORPUS_LOG | wc -l | tr -d ' ')
  f=$(awk -F'\t' -v k=$kind '$1==k && $3=="fail"' $CORPUS_LOG | wc -l | tr -d ' ')
  s=$(awk -F'\t' -v k=$kind '$1==k && $3=="skip"' $CORPUS_LOG | wc -l | tr -d ' ')
  rate=0
  (( tot - s > 0 )) && rate=$(( 100.0 * p / (tot - s) ))
  printf '%s: total=%d pass=%d fail=%d skip=%d pass_rate=%.1f%%\n' $kind $tot $p $f $s $rate
done
echo "log: $CORPUS_LOG"

# --- symptom breakdown -----------------------------------------------------
# `tests/scala_corpus_report.sh <log>` prints the same thing on its own, so a
# log can be re-read without re-running the corpus.
[[ -n ${CORPUS_NO_REPORT:-} ]] || $ROOT/tests/scala_corpus_report.sh $CORPUS_LOG
