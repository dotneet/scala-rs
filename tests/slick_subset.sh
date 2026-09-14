#!/bin/zsh
# The goal's second half is that slick *runs*. `slick_measure.sh` counts type
# errors and stops there (`classes=0` while any file fails). This script finds
# the fixpoint of files that compile *together* cleanly, emits their classes,
# and then actually loads every emitted class with the verifier on -- the
# first measurement on the "runs" axis.
set -e
ROOT=${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}
source "$ROOT/tests/fixture_cache.sh"
source "$ROOT/tests/slick_subset_errors.sh"
SP=${SLICK_FIXTURE_DIR:-$(fixture_path slick)}
typeset -a FIND_RESULT
find_sorted() {
  FIND_RESULT=( "${(@0)$(find "$@" -print0 | sort -z)}" )
  FIND_RESULT=( "${(@)FIND_RESULT:#}" )
}
SLICK_REV=$(fixture_cfg slick revision)
fixture_require_checkout slick "$SP/slick"
SRC=$SP/slick/slick/src/main
COMPAT=$SP/slick/slick-compat-collections/src/main/scala-2.13+
BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
RUN=$SP/subset-$$
GEN=$RUN/generated
fixture_safe_clean "$SP" "$RUN"; mkdir -p "$RUN"
# Phase timing, so the cost of this script is attributable without deriving it
# from log mtimes. It goes to stderr: callers parse the last lines of stdout
# (`tests/verify_merge.sh` takes `tail -3`), and that contract must not move.
zmodload -i zsh/datetime
PHASE_T0=$EPOCHREALTIME
phase() { printf 'phase %-10s %6.1fs\n' "$1" $(( EPOCHREALTIME - PHASE_T0 )) >&2
          PHASE_T0=$EPOCHREALTIME }
python3 "$ROOT/tests/expand_fm.py" "$SRC/scala" "$GEN" >/dev/null
find_sorted "$SRC/scala" -type f
FM_INPUTS=("${FIND_RESULT[@]}")
FM_DIGEST=$(fixture_source_digest "$FIXTURE_ROOT" "${FM_INPUTS[@]}")
GEN_COUNT=$(find "$GEN" -type f -name '*.scala' | wc -l | tr -d ' ')
GEN_STATE=$(fixture_cfg slick generated_state)
fixture_validate_generated slick "$GEN_COUNT" "$GEN_STATE"
GEN_CONTENT_DIGEST=$(fixture_generated_content_digest "$GEN" "$GEN")
fixture_generated_mark slick "$GEN" "$SLICK_REV" \
  "$FM_DIGEST" \
  "$GEN_COUNT" "$GEN_STATE" "$GEN_CONTENT_DIGEST"
REFLECT=$(fixture_toolchain_reflect)
DEPS_MARK=$SP/.fixture-deps
DEPS_CONTENT_DIGEST=$(fixture_dependency_digest "$SP/deps.cp" 2>/dev/null || print missing)
fixture_state_valid "$DEPS_MARK" slick "$SLICK_REV" "$SLICK_REV" "$DEPS_CONTENT_DIGEST" classpath || {
  print -u2 "slick dependency classpath is stale or unmarked; run tests/slick_measure.sh once first"
  exit 1
}
CP="$(cat "$SP/deps.cp"):$REFLECT"
fixture_validate_classpath "$CP"
LIB=$(fixture_toolchain_library)
JAVA_BIN=$(fixture_java_path)
JAVAC_BIN=$(fixture_javac_path)
find_sorted "$SRC/scala" "$SRC/scala-2" "$COMPAT" "$GEN" -name '*.scala'
FILES=("${FIND_RESULT[@]}")
fixture_validate_source_count slick "${#FILES[@]}" default
CONVERGED=0
# Round 1 compiles exactly what `slick_measure.sh` just compiled -- all 184
# files, same flags -- and that pass costs 4.5 minutes. When a fresh measure
# log is available, start from its verdict instead. `SLICK_SEED_LOG` is set by
# the verification pipeline; without it the loop runs from scratch as before.
if [[ -n ${SLICK_SEED_LOG:-} && -s ${SLICK_SEED_LOG:-/nonexistent} ]]; then
  if grep -q "panicked at" "$SLICK_SEED_LOG"; then
    echo "COMPILER PANIC in the seed measurement (not a fixpoint):" >&2
    grep -m1 "panicked at" "$SLICK_SEED_LOG" >&2
    exit 1
  fi
  # Only *errors* evict a file. A `-->` line also follows every **warning**,
  # and taking those too threw out `JdbcActionComponent.scala` on a clean
  # (0-error, 2-warning) measurement -- which then made the files that depend
  # on it fail, and the loop shrank a converged set from 184 to 132.
  SEED_PARSE_RC=0
  slick_subset_parse_errors "$SLICK_SEED_LOG" "$RUN/seed_bad.txt" || SEED_PARSE_RC=$?
  if (( SEED_PARSE_RC == 2 )); then
    print -u2 "cannot parse slick seed diagnostics (errors=$SLICK_SUBSET_ERROR_COUNT locations=$SLICK_SUBSET_LOCATION_COUNT)"
    grep -m8 '^' "$SLICK_SEED_LOG" >&2
    exit 1
  fi
  if (( SEED_PARSE_RC == 0 )); then
    typeset -a BAD_FILES NEXT_FILES
    BAD_FILES=()
    while IFS= read -r bad; do BAD_FILES+=("$bad"); done < "$RUN/seed_bad.txt"
    NEXT_FILES=()
    for path in "${FILES[@]}"; do
      keep=1
      for bad in "${BAD_FILES[@]}"; do
        [[ $path == "$bad" ]] && { keep=0; break; }
      done
      (( keep )) && NEXT_FILES+=("$path")
    done
    FILES=("${NEXT_FILES[@]}")
    if (( ${#FILES[@]} == 0 )); then
      print -u2 "seed diagnostics rejected every slick source"
      exit 1
    fi
  fi
fi
for round in 1 2 3 4 5 6 7 8; do
  OUT=$RUN/out; fixture_safe_clean "$RUN" "$OUT"; mkdir -p "$OUT"
  COMPILER_STATUS=0
  "$BIN" compile "${FILES[@]}" -d "$OUT" -cp "$CP" -Xsource:3 \
    --scala-library "$LIB" > "$RUN/log.txt" 2>&1 || COMPILER_STATUS=$?
  # Files named in any error line leave the set; repeat until none are.
  # A panic prints no `error:` lines and no `-->`, which used to read as
  # "converged, clean" -- round 2 once reported 126 files / 0 classes off a
  # crash in file one. A crash is a compiler bug, never a fixpoint.
  if grep -q "panicked at" "$RUN/log.txt"; then
    echo "COMPILER PANIC (not a fixpoint):" >&2
    grep -m1 "panicked at" "$RUN/log.txt" >&2
    exit 1
  fi
  PARSE_RC=0
  slick_subset_parse_errors "$RUN/log.txt" "$RUN/bad.txt" || PARSE_RC=$?
  if (( PARSE_RC == 1 )); then
    if (( COMPILER_STATUS != 0 )); then
      print -u2 "compiler exited $COMPILER_STATUS without parseable type-error diagnostics (not a fixpoint)"
      grep -m8 '^' "$RUN/log.txt" >&2
      exit 1
    fi
    CONVERGED=1
    break
  fi
  if (( PARSE_RC != 0 )); then
    print -u2 "cannot parse compiler diagnostics (exit=$COMPILER_STATUS errors=$SLICK_SUBSET_ERROR_COUNT locations=$SLICK_SUBSET_LOCATION_COUNT)"
    grep -m8 '^' "$RUN/log.txt" >&2
    exit 1
  fi
  if (( COMPILER_STATUS != 1 )); then
    print -u2 "compiler returned $COMPILER_STATUS for type-error diagnostics (expected exit 1)"
    grep -m8 '^' "$RUN/log.txt" >&2
    exit 1
  fi
  BAD_FILES=()
  while IFS= read -r bad; do BAD_FILES+=("$bad"); done < "$RUN/bad.txt"
  NEXT_FILES=()
  for path in "${FILES[@]}"; do
    keep=1
    for bad in "${BAD_FILES[@]}"; do
      [[ $path == "$bad" ]] && { keep=0; break; }
    done
    (( keep )) && NEXT_FILES+=("$path")
  done
  if (( ${#NEXT_FILES[@]} == ${#FILES[@]} )); then
    print -u2 "compiler diagnostics did not name an input source (exit=$COMPILER_STATUS)"
    grep -m8 '^' "$RUN/log.txt" >&2
    exit 1
  fi
  FILES=("${NEXT_FILES[@]}")
  if (( ${#FILES[@]} == 0 )); then
    print -u2 "compiler diagnostics rejected every slick source (exit=$COMPILER_STATUS)"
    exit 1
  fi
done
(( CONVERGED )) || {
  print -u2 "slick subset did not converge after 8 compile rounds"
  exit 1
}
phase compile
NFILES=${#FILES[@]}
NCLASSES=$(find "$RUN/out" -name '*.class' | wc -l | tr -d ' ')
# Load every class with verification on. Class.forName(initialize=false)
# still runs the bytecode verifier, without executing initializers.
cat > "$RUN/V.java" <<'JAVA'
import java.io.*; import java.nio.file.*; import java.net.*;
public class V {
  public static void main(String[] a) throws Exception {
    Path root = Paths.get(a[0]);
    URLClassLoader cl = new URLClassLoader(new URL[]{root.toUri().toURL(),
      new File(a[1]).toURI().toURL()}, V.class.getClassLoader());
    int ok = 0; int bad = 0;
    var it = Files.walk(root).filter(p -> p.toString().endsWith(".class")).iterator();
    while (it.hasNext()) {
      Path p = it.next();
      String n = root.relativize(p).toString().replace(".class","").replace(File.separatorChar,'.');
      try { Class.forName(n, false, cl); ok++; }
      catch (Throwable t) { bad++; System.out.println("BAD " + n + " : " + t); }
    }
    System.out.println("verified=" + ok + " failed=" + bad);
  }
}
JAVA
(cd "$RUN" && "$JAVAC_BIN" V.java >/dev/null 2>&1)
"$JAVA_BIN" -Xverify:all -cp "$RUN:$RUN/out:$LIB:$CP" V "$RUN/out" "$LIB" 2>&1 | tail -5
phase load
# The loader above stops after the constant pool, so no method body is looked
# at. This reads the bodies back with `javap -c` and reports the offsets that
# cannot be right -- a branch out of its own method, a method over 64 KB.
# `LINT_JOBS` fans the `javap` chunks out over processes; see classfile_lint.py.
LINT_JOBS=${SUBSET_JOBS:-${LINT_JOBS:-1}} python3 "$ROOT/tests/classfile_lint.py" "$RUN/out" | tail -20
phase lint
echo "subset_files=$NFILES classes=$NCLASSES (of 184 sources)"
fixture_safe_clean "$SP" "$RUN"
