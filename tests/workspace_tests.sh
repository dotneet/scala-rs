#!/bin/zsh
# Run the whole workspace's tests with the test *binaries* in parallel.
#
# Why this exists. `cargo test` builds every test target and then runs them
# ONE BINARY AT A TIME, filling each with `RUST_TEST_THREADS` threads. Before
# the CLI suite was grouped into eight shards, the workspace had more than 300
# tiny test binaries, so the fan-out inside a binary had almost nothing to work
# with while the machine sat idle between binaries. Measured on the batch/w3 tip, the per-binary
# times reported by libtest sum to 1862 s at `RUST_TEST_THREADS=6` and 1850 s
# at 12 -- and in both cases the step's wall time was that same ~31 minutes.
# Raising the in-binary fan-out bought nothing because the binaries, not the
# tests inside them, were the serial part. Sharding removed that compile/link
# pathology; this runner remains the fail-closed, parallel full-suite entrypoint
# for the eight CLI shards and the other workspace test targets.
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
#   WT_TIMEOUT  wall-clock seconds per test binary (default 1800). Set to 0 only
#               when deliberately disabling the guard; invalid values fail
#               before any tests start. A timeout is a missing/nonzero result.
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
TIMEOUT=${WT_TIMEOUT:-1800}
DIR=${WT_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/scala-rs-wt-XXXXXX")}
TIMEOUT_HELPER=$ROOT/tests/run_with_timeout.pl
if [[ $TIMEOUT != <-> ]]; then
  print "workspace_tests: WT_TIMEOUT must be a non-negative integer (got ${(q)TIMEOUT})" >&2
  print "workspace_tests: binaries=0 rows=0 missing=0 failed_bins=0 doc_rows=0 (INVALID WT_TIMEOUT)"
  exit 2
fi
if [[ ! -r $TIMEOUT_HELPER ]]; then
  print "workspace_tests: timeout helper is missing: $TIMEOUT_HELPER" >&2
  print "workspace_tests: binaries=0 rows=0 missing=0 failed_bins=0 doc_rows=0 (TIMEOUT HELPER FAILED)"
  exit 2
fi
mkdir -p "$DIR/out"
T0=$EPOCHREALTIME

# --- build ------------------------------------------------------------------
# `--no-run` builds every test target and names the executables in the JSON
# stream. Nothing else may be using the cargo target lock at this point.
if ! cargo test --workspace --release --no-run --message-format=json \
       > "$DIR/build.json" 2> "$DIR/build.log"; then
  cat "$DIR/build.log" >&2
  print "workspace_tests: binaries=0 rows=0 missing=0 failed_bins=0 doc_rows=0 (BUILD FAILED, see $DIR/build.log)"
  exit 2
fi
# bins.txt is one executable path per line for inspection and fixed-order
# collection. bins0 is the NUL-delimited source for xargs: executable paths can
# contain spaces, so the default whitespace parser is not safe here. The
# package directory of each one is written to pkg/<basename>:
# cargo runs a test binary with its cwd set to the package root, and these
# tests resolve their fixtures through env!("CARGO_MANIFEST_DIR") rather than
# the cwd, but reproducing it costs nothing and removes one way for this runner
# to differ from `cargo test`.
mkdir -p "$DIR/pkg"
python3 - "$DIR/build.json" "$DIR/pkg" "$DIR/bins.txt" "$DIR/bins0" <<'PY'
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
with open(sys.argv[3], "w") as text_out, open(sys.argv[4], "wb") as nul_out:
    for exe, pkg in sorted(out):
        text_out.write(exe + "\n")
        nul_out.write(exe.encode() + b"\0")
        with open(os.path.join(sys.argv[2], os.path.basename(exe)), "w") as f:
            f.write(pkg + "\n")
PY
NBINS=$(wc -l < "$DIR/bins.txt" | tr -d ' ')
if (( NBINS == 0 )); then
  print "workspace_tests: binaries=0 rows=0 missing=0 failed_bins=0 doc_rows=0 (no test binaries found)"
  exit 2
fi

# --- longest first ----------------------------------------------------------
# `--list` is a few milliseconds per binary and gives the only cheap proxy for
# cost there is: how many tests it holds. Without it the 460-test binary can be
# scheduled last and becomes the tail of the whole step.
cat > "$DIR/count.sh" <<'SH'
#!/bin/zsh
b=$1
n=$("$b" --list --format=terse 2>/dev/null | grep -c ': test')
printf '%s\t%s\n' "${n:-0}" "$b"
SH
chmod +x "$DIR/count.sh"
xargs -0 -P 8 -n 1 "$DIR/count.sh" < "$DIR/bins0" 2>/dev/null \
  | sort -t$'\t' -k1,1nr -k2,2 | cut -f2- > "$DIR/order.txt"
# If anything went wrong with the counting, fall back to the plain order: the
# schedule is an optimisation, never a correctness condition.
if (( $(wc -l < "$DIR/order.txt" | tr -d ' ') != NBINS )); then
  print "workspace_tests: note: --list enumeration incomplete, running in path order" >&2
  cp "$DIR/bins.txt" "$DIR/order.txt"
fi
# Convert the one-path-per-line schedule to NUL-delimited input for xargs.
# Keeping this conversion explicit makes the path-boundary contract visible
# and avoids depending on a non-portable xargs -d extension.
python3 - "$DIR/order.txt" "$DIR/order0" <<'PY'
import sys
with open(sys.argv[1], encoding="utf-8") as source, open(sys.argv[2], "wb") as dest:
    for line in source:
        dest.write(line.rstrip("\n").encode() + b"\0")
PY

# --- run --------------------------------------------------------------------
cat > "$DIR/one.sh" <<'SH'
#!/bin/zsh
b=$1
o=$WT_OUT/${b:t}
pkg=$(cat "$WT_PKG/${b:t}" 2>/dev/null)
print "     Running $b" > "$o.log"
cd "${pkg:-.}"
# Pass the binary through a fixed shell command so Perl's multi-argument
# exec path never falls back to its one-string shell form for a path with
# spaces. "$1" remains one argv element all the way to the test binary.
RUST_TEST_THREADS=$WT_THREADS perl "$WT_TIMEOUT_HELPER" "$WT_TIMEOUT" \
  /bin/sh -c 'exec "$1"' sh "$b" >> "$o.log" 2>&1
print -r -- $? > "$o.rc"
SH
chmod +x "$DIR/one.sh"
WT_OUT=$DIR/out WT_PKG=$DIR/pkg WT_THREADS=$THREADS WT_TIMEOUT=$TIMEOUT \
  WT_TIMEOUT_HELPER=$TIMEOUT_HELPER \
  xargs -0 -P "$JOBS" -n 1 "$DIR/one.sh" < "$DIR/order0"
BUILD_AND_RUN=$(printf '%.1f' $(( EPOCHREALTIME - T0 )))

# --- collect, in a fixed order ---------------------------------------------
MISSING=(); FAILED=()
while read -r b; do
  o=$DIR/out/${b:t}
  cat "$o.log"
  grep -q '^test result:' "$o.log" || MISSING+=("${b:t}")
  [[ $(cat "$o.rc" 2>/dev/null || print 1) == 0 ]] || FAILED+=("${b:t}")
done < <(sort "$DIR/bins.txt")
ROWS=$(cat "$DIR/out"/*.log | grep -c '^test result:')

# --- doc tests --------------------------------------------------------------
# `--no-run` does not build these, so cargo runs them. They are a few seconds.
DOCROWS=0
if [[ ${WT_NO_DOC:-0} != 1 ]]; then
  cargo test --workspace --release --doc --no-fail-fast > "$DIR/doc.log" 2>&1
  DOCRC=$?
  cat "$DIR/doc.log"
  DOCROWS=$(grep -c '^test result:' "$DIR/doc.log")
  (( DOCRC == 0 )) || FAILED+=("doc-tests")
fi

print ""
print "workspace_tests: binaries=$NBINS rows=$((ROWS + DOCROWS)) missing=${#MISSING[@]} failed_bins=${#FAILED[@]} doc_rows=$DOCROWS"
print "workspace_tests: jobs=$JOBS threads=$THREADS wall=${BUILD_AND_RUN}s(build+run) dir=$DIR"
for m in "${MISSING[@]}"; do print "workspace_tests: MISSING RESULT $m"; done
for f in "${FAILED[@]}"; do print "workspace_tests: NONZERO EXIT $f"; done
(( ${#MISSING[@]} == 0 && ${#FAILED[@]} == 0 ))
