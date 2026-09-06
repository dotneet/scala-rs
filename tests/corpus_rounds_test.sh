#!/usr/bin/env bash
# Prove that a directory with *_N.scala files is compiled one round at a time.
# This is deliberately a worker-mode test: it does not fetch or run the corpus.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
LOG_ROOT=${CONFORMANCE_LOG_ROOT:-/tmp/scala-rs-codex/conformance-audit}
mkdir -p "$LOG_ROOT"
WORK=$(mktemp -d "$LOG_ROOT/rounds.XXXXXX")
trap 'rm -rf "$WORK"' EXIT

SRC="$WORK/t8944"
BIN="$WORK/fake-scala-rs"
FAKE_C="$WORK/fake-scala-rs.c"
LOG="$WORK/argv.log"
RESULT="$WORK/result.tsv"
mkdir -p "$SRC" "$RESULT.part" "$WORK/out"

printf 'class A1\n' > "$SRC/A_1.scala"
printf 'class A2\n' > "$SRC/A_2.scala"
printf 'class Test\n' > "$SRC/Test_1.scala"

cat > "$FAKE_C" <<'EOF'
#include <errno.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

static int has_suffix(const char *s, const char *suffix) {
  size_t n = strlen(s), m = strlen(suffix);
  return n >= m && strcmp(s + n - m, suffix) == 0;
}

int main(int argc, char **argv) {
  const char *log_path = getenv("ROUND_LOG");
  const char *out = NULL;
  FILE *log;
  int i;
  if (!log_path || !(log = fopen(log_path, "a"))) return 90;
  for (i = 1; i < argc; ++i) {
    if (i > 1) fputc(' ', log);
    fputs(argv[i], log);
    if (strcmp(argv[i], "-d") == 0 && i + 1 < argc) out = argv[++i];
  }
  fputc('\n', log);
  fclose(log);
  if (!out) return 91;
  if (mkdir(out, 0755) != 0 && errno != EEXIST) return 92;
  for (i = 1; i < argc; ++i) {
    const char *arg = argv[i], *base;
    char name[PATH_MAX], class_path[PATH_MAX];
    size_t len;
    FILE *class_file;
    if (!has_suffix(arg, ".scala")) continue;
    base = strrchr(arg, '/');
    base = base ? base + 1 : arg;
    len = strlen(base) - strlen(".scala");
    if (len + 7 >= sizeof(name)) return 93;
    memcpy(name, base, len);
    memcpy(name + len, ".class", 7);
    if (snprintf(class_path, sizeof(class_path), "%s/%s", out, name) >= (int)sizeof(class_path)) return 94;
    class_file = fopen(class_path, "w");
    if (!class_file) return 95;
    fclose(class_file);
  }
  return 0;
}
EOF
${CC:-cc} "$FAKE_C" -o "$BIN"
DIRECT_LOG="$WORK/direct.log"
ROUND_LOG="$DIRECT_LOG" "$BIN" compile "$SRC/A_1.scala" -d "$WORK/direct-out"
[[ $(wc -l < "$DIRECT_LOG" | tr -d ' ') == 1 ]]
[[ -f "$WORK/direct-out/A_1.class" ]]

CORPUS_WORK="$WORK/work" CORPUS_LOG="$RESULT" SCALA_RS="$BIN" \
  ROUND_LOG="$LOG" CORPUS_TIMEOUT=10 \
  "$ROOT/tests/scala_corpus.sh" --one "pos:$SRC"

[[ $(wc -l < "$LOG" | tr -d ' ') == 2 ]]
first=$(sed -n '1p' "$LOG")
second=$(sed -n '2p' "$LOG")
[[ $first == *"$SRC/A_1.scala"* && $first == *"$SRC/Test_1.scala"* ]]
[[ $first != *"$SRC/A_2.scala"* ]]
[[ $second == *"$SRC/A_2.scala"* ]]
[[ $second != *"$SRC/A_1.scala"* && $second != *"$SRC/Test_1.scala"* ]]
grep -q $'pos\tt8944\tpass' "$RESULT.part/pos-t8944"

echo 'corpus rounds: separate invocations and source grouping passed'
