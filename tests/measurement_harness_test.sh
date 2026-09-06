#!/usr/bin/env bash
# Test measurement failure reporting without building the compiler or corpus.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

expect_exit() {
  local expected=$1 actual=0
  shift
  "$@" >"$WORK/result.log" 2>&1 || actual=$?
  if [[ $actual -ne $expected ]]; then
    cat "$WORK/result.log" >&2
    echo "expected exit $expected, got $actual: $*" >&2
    exit 1
  fi
}

for shell in bash zsh; do
  for row in '0 0 0 12 5' '0 1 7 0 5' '2 2 0 0 5' \
             '2 134 3 0 5' '2 1 0 0 5' '2 0 0 0 5' \
             '2 0 1 12 5' '2 0 0 12 0'; do
    read -r expected rc errors classes files <<< "$row"
    expect_exit "$expected" "$shell" -c \
      'source "$1"; validate_measure_result "$2" "$3" "$4" "$5" fixture.log' \
      -- "$ROOT/tests/measure_result.sh" "$rc" "$errors" "$classes" "$files"
  done
done

mkdir -p "$WORK/good" "$WORK/missing" "$WORK/bad" "$WORK/empty" "$WORK/init"
cat > "$WORK/Good.java" <<'JAVA'
public class Good { public static int value() { return 42; } }
JAVA
cat > "$WORK/Child.java" <<'JAVA'
class Parent {}
public class Child extends Parent {}
JAVA
cat > "$WORK/Init.java" <<'JAVA'
public class Init {
  static { if (System.nanoTime() != 0) throw new RuntimeException("fixture"); }
}
JAVA
javac -d "$WORK/good" "$WORK/Good.java"
javac -d "$WORK/missing" "$WORK/Child.java"
javac -d "$WORK/init" "$WORK/Init.java"
rm "$WORK/missing/Parent.class"
printf 'invalid classfile' > "$WORK/bad/Bad.class"

expect_exit 0 bash "$ROOT/tests/verify_all.sh" "$WORK/good"
grep -q 'verify_loaded=1 verify_incomplete=0' "$WORK/result.log"
expect_exit 2 bash "$ROOT/tests/verify_all.sh" "$WORK/missing"
grep -q 'INCOMPLETE Child :: NoClassDefFoundError' "$WORK/result.log"
expect_exit 2 bash "$ROOT/tests/verify_all.sh" "$WORK/init"
grep -q 'INCOMPLETE Init :: ExceptionInInitializerError' "$WORK/result.log"
expect_exit 2 bash "$ROOT/tests/verify_all.sh" "$WORK/empty"
expect_exit 1 bash "$ROOT/tests/verify_all.sh" "$WORK/bad"
grep -q 'verify_failures=1' "$WORK/result.log"
echo 'measurement harness: 21 checks passed'
