#!/bin/zsh
# The specialization ledger: for scala/scala's own `pos/spec-*` tests, compare
# the set of classfile *names* we emit against the set real scalac emits.
#
# Why this exists
# ---------------
# Accepting `@specialized` (stage 1, see docs/specialization.md) turns a large
# number of corpus tests green without implementing specialization: a `pos`
# test only asserts that the program type-checks, and `@specialized` changes no
# answer, only the boxing and the classes on disk. So the `pos` number goes up
# by more than the work earns.
#
# The fix is not to hide that number but to publish one beside it that stage 1
# cannot move. `class Foo[@specialized T]` makes scalac emit `Foo$mcI$sp` and
# eight siblings; we emit `Foo` alone. This script says so, in one line, and
# will keep saying so until the `specialize` phase exists.
#
# What it proves, and what it does not
# ------------------------------------
# It compares *names*, nothing else. It does not load, verify or run either
# side's classfiles, and it says nothing about whether the bodies inside the
# specialized classes are right. It is the cheapest check that fails on exactly
# the thing stage 1 does not do.
#
# Usage:
#   tests/spec_classfiles.sh                 # all pos/spec-* tests
#   SPEC_FILTER=simple tests/spec_classfiles.sh
#
# Environment:
#   SPEC_LOG      per-test TSV (default: a shared scratchpad path -- override it)
#   SPEC_FILTER   substring of the test name to restrict to
#   SPEC_JOBS     parallel workers (default 6)
#   SCALAC        real scalac (default /tmp/scala-2.13.16/bin/scalac)
#   SCALA_RS      use this binary instead of building target/release/scala-rs
set -e

SP=/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/specclasses
# v2.13.16 -- the same release as the scalac we diff against, so a difference
# is about this compiler and not about a version skew.
SCALA_REV=3f6bdaeafde17d790023cc3f299b81eaaf876ca3
CORPUS=${CORPUS_DIR:-/tmp/scala-rs-corpus/scala}
LIB=/tmp/scala-rs-lib/scala-library-2.13.16.jar
CCACHE=$HOME/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2

# ---------------------------------------------------------------------------
# Worker mode: `$0 --one <path>` compiles one test twice and appends one TSV
# line. Everything it needs comes from the environment, which xargs passes on.
# ---------------------------------------------------------------------------
if [[ $1 == --one ]]; then
  src=$2
  name=${src:t:r}
  work=$SPEC_WORK/$name
  rm -rf $work; mkdir -p $work/ours $work/theirs

  emit() {  # verdict ours scalac missing extra sp_ours sp_scalac note
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
      $name $1 $2 $3 $4 $5 $6 $7 "${8:-}" >> $SPEC_LOG.part/$name
  }
  names_in() { (cd $1 && find . -name '*.class' | sed 's,^\./,,' | sort) }

  set +e
  $SCALAC -d $work/theirs $src > $work/scalac.log 2>&1
  scrc=$?
  $SCALA_RS compile $src -d $work/ours --scala-library $LIB > $work/ours.log 2>&1
  ourrc=$?
  set -e

  if (( scrc != 0 )); then
    # Not our failure to report: without scalac's answer there is nothing to
    # diff against, so the test is dropped rather than counted either way.
    emit skip 0 0 0 0 0 0 "scalac failed"
    exit 0
  fi

  theirs=$(names_in $work/theirs)
  ours=$(names_in $work/ours)
  nt=$(print -r -- $theirs | grep -c . || true)
  no=$(print -r -- $ours   | grep -c . || true)
  # `$sp` in a class name is the specialized-class marker (`Foo$mcI$sp`).
  spt=$(print -r -- $theirs | grep -c '\$sp\.class$' || true)
  spo=$(print -r -- $ours   | grep -c '\$sp\.class$' || true)
  missing=$(comm -23 <(print -r -- $theirs) <(print -r -- $ours) | grep -c . || true)
  extra=$(comm -13 <(print -r -- $theirs) <(print -r -- $ours) | grep -c . || true)

  if (( ourrc != 0 )); then
    why=$(grep -m1 '^error' $work/ours.log | tr '\t' ' ' | cut -c1-100)
    emit no-compile 0 $nt $nt 0 0 $spt "${why:-compile failed}"
  elif (( missing == 0 && extra == 0 )); then
    emit match $no $nt 0 0 $spo $spt
  else
    emit differ $no $nt $missing $extra $spo $spt
  fi
  rm -rf $work
  exit 0
fi

# ---------------------------------------------------------------------------
# Driver
# ---------------------------------------------------------------------------
if [[ ! -f $LIB ]]; then
  mkdir -p /tmp/scala-rs-lib
  cp $CCACHE/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar /tmp/scala-rs-lib/
fi
if [[ ! -d $CORPUS/test/files ]]; then
  mkdir -p ${CORPUS:h}; rm -rf $CORPUS
  git clone --depth 1 --branch v2.13.16 --filter=blob:none --no-tags \
      https://github.com/scala/scala.git $CORPUS >/dev/null 2>&1
fi
have=$(git -C $CORPUS rev-parse HEAD)
if [[ $have != $SCALA_REV ]]; then
  echo "corpus revision mismatch: got $have, expected $SCALA_REV (v2.13.16)" >&2
  exit 2
fi

export SCALAC=${SCALAC:-/tmp/scala-2.13.16/bin/scalac}
if [[ ! -x $SCALAC ]]; then
  echo "no scalac at $SCALAC: this ledger is a diff against scalac and cannot" >&2
  echo "be scored without it. Set SCALAC to a 2.13.16 scalac." >&2
  exit 1
fi

ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
export SCALA_RS=${SCALA_RS:-$ROOT/target/release/scala-rs}
# The release binary is not what `cargo test` builds; measuring a stale one
# silently reports the previous commit's numbers.
if [[ -z ${SCALA_RS_PREBUILT:-} ]]; then
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>/tmp/spec_classfiles_build.log \
    || { cat /tmp/spec_classfiles_build.log; exit 1; }
fi

export SPEC_LOG=${SPEC_LOG:-$SP/ledger.tsv}
export SPEC_WORK=${SPEC_WORK:-$SP/work-$$}
mkdir -p ${SPEC_LOG:h} $SPEC_WORK
rm -rf $SPEC_LOG.part; mkdir -p $SPEC_LOG.part
: > $SPEC_LOG

tests=($CORPUS/test/files/pos/spec-*.scala(N))
if [[ -n ${SPEC_FILTER:-} ]]; then
  tests=(${(M)tests:#*${~SPEC_FILTER}*})
fi
if (( ${#tests} == 0 )); then
  echo "no pos/spec-* tests under $CORPUS" >&2
  exit 1
fi

expected_total=${#tests}
set +e
print -l $tests | xargs -P ${SPEC_JOBS:-6} -n 1 -I{} $0 --one {}
xargs_rc=$?
set -e
parts=($SPEC_LOG.part/*(N))
if (( ${#parts} > 0 )); then
  cat $parts | sort > $SPEC_LOG
else
  : > $SPEC_LOG
fi
actual_total=$(wc -l < $SPEC_LOG | tr -d ' ')
if (( xargs_rc != 0 || actual_total != expected_total )); then
  echo "specialization ledger incomplete: expected_rows=$expected_total actual_rows=$actual_total worker_exit=$xargs_rc" >&2
  exit 2
fi
rm -rf $SPEC_LOG.part $SPEC_WORK

tot=$(wc -l < $SPEC_LOG | tr -d ' ')
match=$(awk -F'\t' '$2=="match"' $SPEC_LOG | wc -l | tr -d ' ')
differ=$(awk -F'\t' '$2=="differ"' $SPEC_LOG | wc -l | tr -d ' ')
nocomp=$(awk -F'\t' '$2=="no-compile"' $SPEC_LOG | wc -l | tr -d ' ')
skip=$(awk -F'\t' '$2=="skip"' $SPEC_LOG | wc -l | tr -d ' ')
spo=$(awk -F'\t' '{s+=$7} END {print s+0}' $SPEC_LOG)
spt=$(awk -F'\t' '{s+=$8} END {print s+0}' $SPEC_LOG)
missing=$(awk -F'\t' '{s+=$5} END {print s+0}' $SPEC_LOG)

printf 'tests=%d match=%d differ=%d no_compile=%d skip=%d\n' \
  $tot $match $differ $nocomp $skip
printf 'classfiles scalac emits that we do not: %d\n' $missing
printf 'specialized classes ($sp): scalac=%d scala-rs=%d\n' $spt $spo
if (( match == tot - skip && spo > 0 )); then
  echo "LEDGER: GREEN -- every spec-* test emits scalac's classfile set"
else
  echo "LEDGER: RED -- specialization is not implemented (stage 1 only)"
fi
echo "log: $SPEC_LOG"
