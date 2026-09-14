#!/bin/zsh
# Small smoke test for tests/run_with_timeout.pl. It exercises exit-status
# preservation, the timeout marker, and cleanup of a descendant in the
# timed-out process group without building the Rust workspace.
set -u
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
HELPER=$ROOT/tests/run_with_timeout.pl
[[ -r $HELPER ]] || { print -u2 "missing timeout helper: $HELPER"; exit 1; }

DIR=$(mktemp -d "${TMPDIR:-/tmp}/scala-rs-timeout-test-XXXXXX")
trap 'rm -rf "$DIR"' EXIT INT TERM

perl "$HELPER" 5 /bin/sh -c 'exit 7' >/dev/null 2>&1
rc=$?
[[ $rc == 7 ]] || { print -u2 "exit status was $rc, want 7"; exit 1; }

SECONDS=0
perl "$HELPER" 1 /bin/sh -c 'trap "" TERM; sleep 30' >$DIR/timeout.log 2>&1
rc=$?
elapsed=$SECONDS
[[ $rc == 124 ]] || { print -u2 "timeout status was $rc, want 124"; exit 1; }
grep -qx 'workspace_tests: TIMEOUT limit=1s' $DIR/timeout.log \
  || { print -u2 "timeout marker missing"; exit 1; }
(( elapsed <= 5 )) || { print -u2 "timeout took ${elapsed}s"; exit 1; }

# The parent exits on TERM while its child ignores TERM. This catches the
# subtle case where waitpid returns before the helper's grace-period KILL.
perl "$HELPER" 1 /bin/sh -c 'trap "exit 0" TERM; (trap "" TERM; sleep 30) & echo $! > "$1"; wait' sh $DIR/child.pid \
  >/dev/null 2>&1
rc=$?
[[ $rc == 124 ]] || { print -u2 "descendant timeout status was $rc, want 124"; exit 1; }
child=$(<$DIR/child.pid)
if kill -0 $child 2>/dev/null; then
  print -u2 "timed-out descendant is still alive: $child"
  exit 1
fi

print "run_with_timeout_test: PASS"
