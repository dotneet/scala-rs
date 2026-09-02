#!/bin/zsh
# Kill scala-rs processes left behind by a killed test run or agent.
#
# A `scala-rs` blocked on the macro engine used to wait forever (fixed:
# expand.rs now times out), but a killed parent can still orphan children
# mid-compile. This reaps anything from this checkout and its worktrees.
#
#   tests/reap_strays.sh          # show what would be killed
#   tests/reap_strays.sh --kill   # kill it
set -e
ROOT=$(cd "$(dirname $0)/.." && pwd)
PATTERN="$ROOT.*target/(debug|release)/(scala-rs|deps/)"
FOUND=$(ps -eo pid,etime,args | grep -E "$PATTERN" | grep -v grep || true)
if [[ -z "$FOUND" ]]; then
  echo "no strays"
  exit 0
fi
echo "$FOUND"
if [[ "${1:-}" == "--kill" ]]; then
  echo "$FOUND" | awk '{print $1}' | xargs -n1 kill -9 2>/dev/null || true
  echo "killed"
else
  echo "(re-run with --kill to stop these)"
fi
