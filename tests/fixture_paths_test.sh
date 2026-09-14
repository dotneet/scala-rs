#!/bin/zsh
# Offline regression check for the NUL-safe path collection used by the
# external-fixture scripts.  In particular, keep this independent of
# fixture_cache.sh so it can run before any fixture or network setup.
set -e

TMP=$(mktemp -d "${TMPDIR:-/tmp}/scala-rs fixture paths.XXXXXX")
trap 'rm -rf -- "$TMP"' EXIT INT TERM
export SCALA_RS_FIXTURE_ROOT="$TMP/fixture root"
SOURCE_ROOT="$SCALA_RS_FIXTURE_ROOT/source tree"
mkdir -p "$SOURCE_ROOT/nested dir"
touch "$SOURCE_ROOT/z file.scala" \
  "$SOURCE_ROOT/a file.scala" \
  "$SOURCE_ROOT/nested dir/middle file.scala"

typeset -a FIND_RESULT
find_sorted() {
  FIND_RESULT=( "${(@0)$(find "$@" -print0 | sort -z)}" )
  FIND_RESULT=( "${(@)FIND_RESULT:#}" )
}

find_sorted "$SOURCE_ROOT" -type f -name '*.scala'
[[ ${#FIND_RESULT[@]} -eq 3 ]]
[[ ${FIND_RESULT[1]} == "$SOURCE_ROOT/a file.scala" ]]
[[ ${FIND_RESULT[2]} == "$SOURCE_ROOT/nested dir/middle file.scala" ]]
[[ ${FIND_RESULT[3]} == "$SOURCE_ROOT/z file.scala" ]]

# The zsh glob form used for Java helpers must retain a whitespace-containing
# prefix and its nullglob semantics.
touch "$SOURCE_ROOT/nested dir/helper file.java"
JAVA_FILES=("$SOURCE_ROOT"/**/*.java(N))
[[ ${#JAVA_FILES[@]} -eq 1 ]]
[[ ${JAVA_FILES[1]} == "$SOURCE_ROOT/nested dir/helper file.java" ]]

for script in \
  tests/cats_measure.sh tests/cats_run.sh \
  tests/gitbucket_measure.sh tests/gitbucket_run.sh \
  tests/slick_measure.sh tests/slick_run.sh tests/slick_subset.sh \
  tests/slick_subset_errors.sh tests/slick_subset_test.sh \
  tests/workspace_tests.sh tests/workspace_tests_test.sh \
  tests/scalalib_measure.sh tests/testkit_measure.sh; do
  zsh -n "$script"
done

if RUNS=invalid tests/slick_run.sh >/dev/null 2>&1; then
  print -u2 "slick_run accepted a non-numeric RUNS value"
  exit 1
else
  rc=$?
  [[ $rc -eq 2 ]] || {
    print -u2 "slick_run returned $rc for an invalid RUNS value (expected 2)"
    exit 1
  }
fi
print -r -- "fixture path expansion ok: $SCALA_RS_FIXTURE_ROOT"
