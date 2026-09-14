#!/bin/zsh
# Run one or more historical CLI integration-test modules after the suite was
# sharded. Each source module still owns its tests; this script only resolves
# the Cargo target that contains it and applies a module-qualified libtest
# filter, so `tests/cli_test.sh vcbridge` does not run the rest of its shard.

set -u
ROOT=${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}
cd "$ROOT"

if (( $# == 0 )); then
  print -u2 "usage: tests/cli_test.sh TEST_MODULE [TEST_MODULE ...]"
  exit 2
fi

overall_rc=0
for module in "$@"; do
  if [[ $module != [A-Za-z0-9_]* || $module == *[^A-Za-z0-9_]* ]]; then
    print -u2 "invalid CLI test module: $module"
    overall_rc=2
    continue
  fi
  matches=()
  for shard in crates/cli/tests/shards/shard_*.rs; do
    grep -Eq "^mod ${module};$" "$shard" && matches+=("$shard")
  done
  if (( ${#matches[@]} != 1 )); then
    print -u2 "CLI test module must occur in exactly one shard: $module (found ${#matches[@]})"
    overall_rc=2
    continue
  fi
  target="cli_${matches[1]:t:r}"
  print "==> $module ($target)"
  cargo test -p scala-rs-cli --release --test "$target" -- "${module}::" || overall_rc=$?
done
exit $overall_rc
