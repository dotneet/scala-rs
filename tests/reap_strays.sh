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

# --- merged worktrees -------------------------------------------------------
# Each agent gets its own worktree with its own `target/`, which is 0.7-1.3 GB
# once it has built. They are not cleaned up when the agent finishes, and 64 of
# them had reached 18 GB before anyone looked. Removing a worktree does not
# remove its branch, so nothing is lost: the commits stay in the repository and
# the directory can be checked out again.
#
#   tests/reap_strays.sh --worktrees          # list merged, clean worktrees
#   tests/reap_strays.sh --worktrees --kill   # remove them
if [[ " $* " == *" --worktrees "* ]]; then
  echo
  echo "=== agent worktrees whose branch is already in main ==="
  n=0
  while IFS=$'\t' read -r w b; do
    br=${b#refs/heads/}
    git merge-base --is-ancestor "$br" main 2>/dev/null || continue
    # Never touch one with uncommitted work, however stale it looks.
    [[ -n $(git -C "$w" status --porcelain 2>/dev/null) ]] && continue
    n=$((n + 1))
    if [[ " $* " == *" --kill "* ]]; then
      git worktree remove --force "$w" && echo "removed $(basename "$w")"
    else
      echo "  $(basename "$w")   $(du -sh "$w" 2>/dev/null | cut -f1)"
    fi
  done < <(git worktree list --porcelain |
             awk '/^worktree /{w=$2} /^branch /{print w"\t"$2}' | grep "/agent-")
  [[ " $* " == *" --kill "* ]] && git worktree prune
  echo "$n worktree(s)"
fi
