#!/bin/zsh
# Run the whole workspace's tests with the test *binaries* in parallel.
#
# Why this exists. `cargo test` builds every test target and then runs them
# ONE BINARY AT A TIME, filling each with `RUST_TEST_THREADS` threads. The
# workspace has 329 test binaries and most of them hold a handful of tests, so
# the fan-out inside a binary has almost nothing to work with while the machine
# sits idle between binaries. Measured on the batch/w3 tip, the per-binary
# times reported by libtest sum to 1862 s at `RUST_TEST_THREADS=6` and 1850 s
# at 12 -- and in both cases the step's wall time was that same ~31 minutes.
# Raising the in-binary fan-out bought nothing because the binaries, not the
# tests inside them, were the serial part.
#
# So: build once with `--no-run`, then run the binaries `WT_JOBS` at a time.
# Longest first, by test count, so the slowest binary is not the one that
# starts last and leaves everything else waiting on its tail.
#
# The output is libtest's own, concatenated in a fixed order (so two runs of
# the same tree print the same thing), which keeps every `test result:` line
# that the merge gate's arithmetic reads. Doc tests are not built by `--no-run`
# and are run afterwards by cargo itself.
#
# Env:
#   WT_JOBS     test binaries in flight (default 6)
#   WT_THREADS  RUST_TEST_THREADS inside each binary (default 4). WT_JOBS x
#               WT_THREADS is the real fan-out: each test thread tends to spawn
#               a `scala-rs` child, and some a `java` macro engine on top.
#   WT_DIR      work directory (default: a fresh one under $TMPDIR)
#   WT_NO_DOC=1 skip the doc tests
#
# Exits non-zero if any binary failed, crashed without reporting a result, or
# the build failed. The last line is always
#   workspace_tests: binaries=N rows=M missing=K failed_bins=F doc_rows=D
# `missing` is the check `cargo test` never had: a binary that dies on a signal
# prints no `test result:` line at all, and summing the lines that are there
# cannot notice the one that is not.
set -u
zmodload -i zsh/datetime
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
cd "$ROOT"
JOBS=${WT_JOBS:-6}
THREADS=${WT_THREADS:-4}
DIR=${WT_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/scala-rs-wt-XXXXXX")}
mkdir -p $DIR/out
T0=$EPOCHREALTIME

# --- build ------------------------------------------------------------------
# `--no-run` builds every test target and names the executables in the JSON
# stream. Nothing else may be using the cargo target lock at this point.
if ! cargo test --workspace --release --no-run --message-format=json \
       > $DIR/build.json 2> $DIR/build.log; then
  cat $DIR/build.log >&2
  print "workspace_tests: binaries=0 rows=0 missing=0 failed_bins=0 doc_rows=0 (BUILD FAILED, see $DIR/build.log)"
  exit 2
fi
# bins.txt is one executable path per line and nothing else, because it is fed
# to xargs. The package directory of each one is written to pkg/<basename>:
# cargo runs a test binary with its cwd set to the package root, and these
# tests resolve their fixtures through env!("CARGO_MANIFEST_DIR") rather than
# the cwd, but reproducing it costs nothing and removes one way for this runner
# to differ from `cargo test`.
mkdir -p $DIR/pkg
python3 - $DIR/build.json $DIR/pkg > $DIR/bins.txt <<'PY'
import json, os, sys
seen, out = set(), []
with open(sys.argv[1]) as f:
    for line in f:
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("reason") != "compiler-artifact":
            continue
        exe = m.get("executable")
        if exe and m.get("profile", {}).get("test") and exe not in seen:
            seen.add(exe)
            out.append((exe, os.path.dirname(m.get("manifest_path", "")) or "."))
for exe, pkg in sorted(out):
    print(exe)
    with open(os.path.join(sys.argv[2], os.path.basename(exe)), "w") as f:
        f.write(pkg + "\n")
PY
NBINS=$(wc -l < $DIR/bins.txt | tr -d ' ')
if (( NBINS == 0 )); then
  print "workspace_tests: binaries=0 rows=0 missing=0 failed_bins=0 doc_rows=0 (no test binaries found)"
  exit 2
fi

# --- longest first ----------------------------------------------------------
# `--list` is a few milliseconds per binary and gives the only cheap proxy for
# cost there is: how many tests it holds. Without it the 460-test binary can be
# scheduled last and becomes the tail of the whole step.
cat > $DIR/count.sh <<'SH'
#!/bin/zsh
b=$1
n=$($b --list --format=terse 2>/dev/null | grep -c ': test')
printf '%s\t%s\n' ${n:-0} $b
SH
chmod +x $DIR/count.sh
xargs -P 8 -n 1 $DIR/count.sh < $DIR/bins.txt 2>/dev/null \
  | sort -t$'\t' -k1,1nr -k2,2 | cut -f2 > $DIR/order.txt
# If anything went wrong with the counting, fall back to the plain order: the
# schedule is an optimisation, never a correctness condition.
if (( $(wc -l < $DIR/order.txt | tr -d ' ') != NBINS )); then
  print "workspace_tests: note: --list enumeration incomplete, running in path order" >&2
  cp $DIR/bins.txt $DIR/order.txt
fi

# --- run --------------------------------------------------------------------
cat > $DIR/one.sh <<'SH'
#!/bin/zsh
b=$1
o=$WT_OUT/${b:t}
pkg=$(cat $WT_PKG/${b:t} 2>/dev/null)
print "     Running $b" > $o.log
cd ${pkg:-.}
RUST_TEST_THREADS=$WT_THREADS $b >> $o.log 2>&1
print -r -- $? > $o.rc
SH
chmod +x $DIR/one.sh
WT_OUT=$DIR/out WT_PKG=$DIR/pkg WT_THREADS=$THREADS \
  xargs -P $JOBS -n 1 $DIR/one.sh < $DIR/order.txt
BUILD_AND_RUN=$(printf '%.1f' $(( EPOCHREALTIME - T0 )))

# --- collect, in a fixed order ---------------------------------------------
MISSING=(); FAILED=()
while read -r b; do
  o=$DIR/out/${b:t}
  cat $o.log
  grep -q '^test result:' $o.log || MISSING+=("${b:t}")
  [[ $(cat $o.rc 2>/dev/null || print 1) == 0 ]] || FAILED+=("${b:t}")
done < <(sort $DIR/bins.txt)
ROWS=$(cat $DIR/out/*.log | grep -c '^test result:')

# --- doc tests --------------------------------------------------------------
# `--no-run` does not build these, so cargo runs them. They are a few seconds.
DOCROWS=0
if [[ ${WT_NO_DOC:-0} != 1 ]]; then
  cargo test --workspace --release --doc --no-fail-fast > $DIR/doc.log 2>&1
  DOCRC=$?
  cat $DIR/doc.log
  DOCROWS=$(grep -c '^test result:' $DIR/doc.log)
  (( DOCRC == 0 )) || FAILED+=("doc-tests")
fi

print ""
print "workspace_tests: binaries=$NBINS rows=$((ROWS + DOCROWS)) missing=${#MISSING[@]} failed_bins=${#FAILED[@]} doc_rows=$DOCROWS"
print "workspace_tests: jobs=$JOBS threads=$THREADS wall=${BUILD_AND_RUN}s(build+run) dir=$DIR"
for m in ${MISSING[@]:-}; do print "workspace_tests: MISSING RESULT $m"; done
for f in ${FAILED[@]:-}; do print "workspace_tests: NONZERO EXIT $f"; done
(( ${#MISSING[@]} == 0 && ${#FAILED[@]} == 0 ))
