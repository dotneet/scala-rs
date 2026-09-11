#!/bin/zsh
# Differential run-time probe: silent miscompilation and wrong acceptance.
#
# Every program in tests/rtprobe/*.scala is an `object Main` that prints what
# it computes. Each one is compiled by scala-rs (`--scala-library`) and by
# real scalac 2.13.16, both results are run under `java -Xverify:all`, and the
# two runs are compared. Verdicts:
#
#   ok            both compilers accept and both runs print the same thing
#   MISCOMPILE    both accept, the runs differ (stdout, or exception/exit code)
#   RUNTIME-ERR   both accept, scalac's run exits cleanly and ours does not
#                 (VerifyError, AbstractMethodError, uncaught exception, ...)
#   WRONG-ACCEPT  scala-rs accepts a program scalac rejects
#   reject        scala-rs rejects a program scalac accepts
#   bad-probe     scalac rejects it and so do we -- a broken probe, fix it
#
# Programs named `neg_*.scala` are the other direction: scalac must reject
# them, and both rejecting is `ok`. scala-rs accepting one is WRONG-ACCEPT;
# scalac accepting one is a bad-probe.
#
# Why it exists: a defect list built from error messages cannot contain the
# defects that produce no message. `Map.updated` answering at the wrong type,
# a `var` inherited from a class file stored by `putfield`, a bounded type
# parameter's missing `checkcast` -- each compiled cleanly and was found only
# by running the output next to scalac's.
#
# Programs catch their own expected exceptions and print them; an uncaught
# exception is compared too (its class, not its stack trace).
#
# Usage: tests/rt_probe.sh [name-regex]
# Env:   SCALA_RS       a binary to measure instead of building target/release
#        RTPROBE_DIR    where outputs and logs go (default: a fresh mktemp dir)
#        RTPROBE_CACHE  scalac results cache, keyed by source hash
#                       (default /tmp/scala-rs-rtprobe-cache; scalac never
#                       changes, so a rerun only pays for scala-rs)
#        RTPROBE_JOBS   parallel workers (default 6)
#        RTPROBE_JAVA   the JVM for both runs (default /usr/bin/java, JDK 17)
#
# Exit status: 0 when every program is `ok`, 1 otherwise.
set -u
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
SCALAC=${SCALAC:-/tmp/scala-2.13.16/bin/scalac}
JAR=${RTPROBE_JAR:-/tmp/scala-rs-lib/scala-library-2.13.16.jar}
CACHE=${RTPROBE_CACHE:-/tmp/scala-rs-rtprobe-cache}
TIMEOUT=${RTPROBE_TIMEOUT:-30}
# Both runs use the same JVM. It is pinned because Float/Double toString
# changed in JDK 19, and the scalac results are cached across runs.
JAVA=${RTPROBE_JAVA:-/usr/bin/java}

# Run "$@" with a wall-clock limit (macOS has no coreutils `timeout`).
limit() { perl -e 'alarm shift; exec @ARGV or die "exec: $!"' $TIMEOUT "$@" }

# Run Main from classpath $1; stdout -> $2, and a normalized status -> $3:
# `exit=N` plus the first line of stderr that names a Throwable, so that two
# runs dying of the same exception compare equal regardless of stack traces.
run_main() {
  local cp=$1 out=$2 st=$3 err=$3.stderr
  limit $JAVA -Xverify:all -XX:-ShowCodeDetailsInExceptionMessages -cp $cp:$JAR Main > $out 2> $err
  local code=$?
  {
    print "exit=$code"
    grep -m1 -E '(Exception|Error|Throwable)' $err | sed -E 's/^Exception in thread "[^"]*" //; s/:.*//' || true
  } > $st
}

# --- worker: one program --------------------------------------------------
if [[ ${1:-} == --one ]]; then
  src=$2 P=$3 BIN=$4
  n=${${src:t}:r}
  W=$P/$n
  rm -rf $W; mkdir -p $W/rs $W/sc
  # scalac side, cached by content hash (and the JVM that ran it).
  key=$( { cat $src; print -- $JAVA } | shasum -a 256 | cut -c1-16)
  C=$CACHE/$n-$key
  if [[ ! -f $C/done ]]; then
    rm -rf $C; mkdir -p $C/cls
    if limit $SCALAC -nowarn -classpath $JAR -d $C/cls $src > $C/compile.log 2>&1; then
      print accept > $C/compiled
      run_main $C/cls $C/out.txt $C/status.txt
    else
      print reject > $C/compiled
    fi
    touch $C/done
  fi
  cp $C/compile.log $W/sc.compile.log
  s=$(<$C/compiled)
  # scala-rs side.
  if limit $BIN compile $src -d $W/rs --scala-library $JAR > $W/rs.compile.log 2>&1; then
    r=accept
  else
    r=reject
  fi
  if [[ $n == neg_* && $s == accept ]]; then
    v=bad-probe
  elif [[ $r == accept && $s == accept ]]; then
    run_main $W/rs $W/rs.out.txt $W/rs.status.txt
    cp $C/out.txt $W/sc.out.txt; cp $C/status.txt $W/sc.status.txt
    if cmp -s $W/rs.out.txt $W/sc.out.txt && cmp -s $W/rs.status.txt $W/sc.status.txt; then
      v=ok
    elif [[ $(head -1 $W/sc.status.txt) == exit=0 && $(head -1 $W/rs.status.txt) != exit=0 ]]; then
      v=RUNTIME-ERR
    else
      v=MISCOMPILE
    fi
  elif [[ $r == accept ]]; then v=WRONG-ACCEPT
  elif [[ $s == accept ]]; then v=reject
  elif [[ $n == neg_* ]]; then v=ok
  else v=bad-probe
  fi
  print $v > $W/verdict
  exit 0
fi

# --- driver ---------------------------------------------------------------
BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
if [[ -z ${SCALA_RS:-} ]]; then
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>/tmp/rt_probe_build.$$.log \
    || { cat /tmp/rt_probe_build.$$.log; exit 1; }
  rm -f /tmp/rt_probe_build.$$.log
fi
for f in $BIN $SCALAC $JAR $JAVA; do
  [[ -e $f ]] || { print "rt_probe: missing $f" >&2; exit 1 }
done
P=${RTPROBE_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/scala-rs-rtprobe-XXXXXX")}
mkdir -p $P $CACHE
FILTER=${1:-.}
srcs=(${(f)"$(ls $ROOT/tests/rtprobe/*.scala | grep -E -- "$FILTER")"})
(( ${#srcs} )) || { print "rt_probe: no programs match $FILTER" >&2; exit 1 }
SELF=${0:A}
start=$SECONDS
print -l -- $srcs | xargs -P ${RTPROBE_JOBS:-6} -I{} zsh $SELF --one {} $P $BIN

typeset -A count
for k in ok MISCOMPILE RUNTIME-ERR WRONG-ACCEPT reject bad-probe; do count[$k]=0; done
for src in $srcs; do
  n=${${src:t}:r}
  v=$(cat $P/$n/verdict 2>/dev/null || print missing)
  (( count[$v]++ ))
  if [[ $v != ok ]]; then
    printf '%-34s %s\n' $n $v
    case $v in
      MISCOMPILE|RUNTIME-ERR)
        diff $P/$n/sc.out.txt $P/$n/rs.out.txt | head -${RTPROBE_DIFF_LINES:-6} | sed 's/^/    /'
        cmp -s $P/$n/sc.status.txt $P/$n/rs.status.txt || {
          print "    scalac: $(tr '\n' ' ' < $P/$n/sc.status.txt)"
          print "    ours:   $(tr '\n' ' ' < $P/$n/rs.status.txt)"
        } ;;
      reject) grep -m2 -E 'error' $P/$n/rs.compile.log | cut -c1-200 | sed 's/^/    /' ;;
      WRONG-ACCEPT|bad-probe) grep -m2 -E 'error' $P/$n/sc.compile.log | cut -c1-200 | sed 's/^/    /' ;;
    esac
  fi
done
print
print "programs=${#srcs} ok=$count[ok] MISCOMPILE=$count[MISCOMPILE] RUNTIME-ERR=$count[RUNTIME-ERR] WRONG-ACCEPT=$count[WRONG-ACCEPT] reject=$count[reject] bad-probe=$count[bad-probe] seconds=$(( SECONDS - start ))"
print "logs=$P"
(( count[ok] == ${#srcs} ))
