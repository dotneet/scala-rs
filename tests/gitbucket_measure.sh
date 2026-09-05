#!/bin/zsh
# Compile gitbucket's main sources with scala-rs and report the error count.
# Usage: tests/gitbucket_measure.sh [extra scala-rs args...]
#
# Modelled on tests/slick_measure.sh and tests/cats_measure.sh. Same rules:
#   * pin the revision, and re-fetch the material when it is missing, so a
#     reboot that wipes /tmp costs one slow run and not a debugging session;
#   * every path this script writes is per-invocation, so two measurements
#     running at once cannot report each other's numbers;
#   * point GITBUCKET_LOG at a path of your own -- the default is shared.
set -e
SP=/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/gitbucket
# master as of 2026-09-04, "Update scala3-library to 3.9.0"; build.sbt says
# version 4.48.0, scalaVersion 2.13.18.
GITBUCKET_REV=3e1e429c8e54ab726a663d9ba14ccf341933adbe
CCACHE=$HOME/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2
if [[ ! -x /tmp/scala-2.13.16/bin/scalac ]]; then
  mkdir -p /tmp/scala-2.13.16/bin /tmp/scala-2.13.16/lib /tmp/scala-rs-lib
  cp $CCACHE/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar /tmp/scala-rs-lib/
  cp $CCACHE/org/scala-lang/scala-library/2.13.16/scala-library-2.13.16.jar /tmp/scala-2.13.16/lib/scala-library.jar
  cp $CCACHE/org/scala-lang/scala-reflect/2.13.16/scala-reflect-2.13.16.jar /tmp/scala-2.13.16/lib/scala-reflect.jar
  cp $CCACHE/org/scala-lang/scala-compiler/2.13.16/scala-compiler-2.13.16.jar /tmp/scala-2.13.16/lib/scala-compiler.jar
  printf '#!/bin/sh\nL=/tmp/scala-2.13.16/lib\nexec java -cp "$L/scala-compiler.jar:$L/scala-library.jar:$L/scala-reflect.jar" scala.tools.nsc.Main "$@"\n' > /tmp/scala-2.13.16/bin/scalac
  chmod +x /tmp/scala-2.13.16/bin/scalac
fi
SRCROOT=$SP/gitbucket
if [[ ! -d $SRCROOT/.git ]]; then
  mkdir -p $SP; rm -rf $SRCROOT
  git clone https://github.com/gitbucket/gitbucket.git $SRCROOT >/dev/null 2>&1
  (cd $SRCROOT && git checkout -q $GITBUCKET_REV)
fi
# gitbucket writes 139 of its 354 sources with the Twirl template compiler
# (sbt-twirl turns src/main/twirl/**/*.scala.html into Scala). Measuring
# without them asks for a source set real scalac never sees, so let sbt write
# them once, and take the dependency classpath from the same run rather than
# hand-listing 100 jars.
TWIRL=$SRCROOT/target/scala-2.13/twirl/main
if [[ ! -d $TWIRL || ! -s $SP/deps.cp ]]; then
  (cd $SRCROOT && sbt -batch -Dsbt.supershell=false \
     "Compile/managedSources" "export Compile/dependencyClasspath") > $SP/sbt.log 2>&1
  # The last line of a successful `export` is the classpath. Drop
  # scala-library: it is passed separately with --scala-library, and sbt
  # resolves 2.13.18 while we link against 2.13.16.
  tail -1 $SP/sbt.log | tr ':' '\n' | grep -v '/scala-library/' | paste -sd: - > $SP/deps.cp
fi
# ---------------------------------------------------------------------------
# Default to *this* checkout's binary, not a fixed path: run from a git
# worktree, a hardcoded path measures the parent repo's build and an agent's
# own changes appear to do nothing.
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
if [[ -z ${SCALA_RS:-} ]]; then
  (cd "$ROOT" && cargo build -p scala-rs-cli --release) >/dev/null 2>/tmp/gitbucket_measure_build.log \
    || { cat /tmp/gitbucket_measure_build.log; exit 1; }
fi
# GITBUCKET_MODULES picks the source set: `scala+twirl` (default, the whole
# Compile configuration), `scala` (hand-written sources only), or `twirl`
# (generated templates only, which do not typecheck without the hand-written
# sources and are only useful for isolating template-specific symptoms).
MODULES=${GITBUCKET_MODULES:-scala+twirl}
DIRS=()
[[ $MODULES == *scala* ]] && DIRS+=($SRCROOT/src/main/scala)
[[ $MODULES == *twirl* ]] && DIRS+=($TWIRL)
# A parse error stops the run before typing, so one unparseable file hides the
# other 353. `controller/PullRequestsController.scala` writes a guard after a
# value definition in a for-comprehension (`name = pullreq...; if hasRole(...)`),
# which nsc desugars by pairing the value with the generator's element and
# filtering the pair -- not implemented, and diagnosed rather than desugared
# wrongly. So it is held out by default and counted as `skipped`. Set
# GITBUCKET_EXCLUDE='' to measure with it in.
EXCLUDE=${GITBUCKET_EXCLUDE-PullRequestsController.scala}
ALL=($(find $DIRS -name '*.scala' | sort))
if [[ -n $EXCLUDE ]]; then
  FILES=(${ALL:#*$EXCLUDE})
else
  FILES=($ALL)
fi
SKIPPED=$(( ${#ALL[@]} - ${#FILES[@]} ))
RUN=${GITBUCKET_RUN:-$SP/run-$$}
OUT=${GITBUCKET_OUT:-$RUN/out}
rm -rf $OUT; mkdir -p $OUT
LOG=${GITBUCKET_LOG:-$SP/measure.txt}
# gitbucket's own scalacOptions, minus the warning settings that only affect
# reporting (-deprecation -feature -Werror -Wunused:imports -Wconf) and the
# optimiser (-opt:l:method). `-Xsource:3-cross` is load-bearing: gitbucket
# cross-builds for Scala 3 and the source relies on 3's rules.
# gitbucket calls slick's `TableQuery` / `mapTo` macros, and running a macro
# implementation needs `scala.reflect.runtime.universe`. Real scalac has it
# because scala-reflect.jar is part of the compiler's own classpath, not the
# project's -- sbt never puts it on gitbucket's. scala-rs has no such classpath
# of its own, so the jar is appended here for the same reason
# `tests/slick_measure.sh` appends it: measuring without it asks for a macro
# expansion nobody could perform.
REFLECT=/tmp/scala-2.13.16/lib/scala-reflect.jar
$BIN compile "${FILES[@]}" -d $OUT -cp "$(cat $SP/deps.cp):$REFLECT" -Xsource:3-cross \
  -language:postfixOps \
  --scala-library /tmp/scala-rs-lib/scala-library-2.13.16.jar "$@" > $LOG 2>&1 || true
ERRORS=$(grep -c '^error' $LOG || true)
CLASSES=$(find $OUT -name '*.class' | wc -l | tr -d ' ')
# Cascades inflate the raw count; files-with-errors is the honest metric.
BADFILES=$(grep -A 2 '^error' $LOG | grep -oE '(src/main|twirl/main)/[^:]*' | sort -u | wc -l | tr -d ' ')
rm -rf $RUN
echo "files=${#FILES[@]} skipped=$SKIPPED errors=$ERRORS files_with_errors=$BADFILES classes=$CLASSES"
