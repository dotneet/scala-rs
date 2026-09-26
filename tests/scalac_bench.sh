#!/bin/zsh
# Compare scala-rs with scalac 2.13.16 on the workloads of
# tests/scalac_bench_gen.py: one fresh compile each, then both programs' first
# lines of output (identity hashes masked).
#
# Usage: tests/scalac_bench.sh DIR KIND...
#   DIR holds the generated kinds (tests/scalac_bench_gen.py DIR).
#   SCALA_RS (default target/release/scala-rs), SCALAC and SCALA_LIBRARY_JAR
#   override the tools. scalac gets a 2 GB heap: its launcher's default
#   -Xmx256M runs out of memory on the larger kinds.
set -u
ROOT=${0:A:h:h}
RS=${SCALA_RS:-$ROOT/target/release/scala-rs}
SC=${SCALAC:-/tmp/scala-2.13.16/bin/scalac}
LIB=${SCALA_LIBRARY_JAR:-/tmp/scala-rs-lib/scala-library-2.13.16.jar}
DIR=$1; shift
wall() { awk '/^real/ { print $2 }' "$1" }
first_lines() { java -cp "$1:$LIB" Main 2>&1 | head -3 | sed -E 's/(\$Lambda\$[0-9]+)?\/?0x[0-9a-f]+@[0-9a-f]+//g' }
printf '| %-16s | %8s | %8s | %5s | %s |\n' kind scalac scala-rs ratio output
for k in "$@"; do
  OUT=$DIR/out/$k
  rm -rf "$OUT"; mkdir -p "$OUT/scalac" "$OUT/scala-rs"
  /usr/bin/time -p env JAVA_OPTS="-Xmx2g -Xss8m" "$SC" -nowarn -d "$OUT/scalac" "$DIR/$k"/*.scala > "$OUT/scalac.log" 2>&1
  sc_rc=$?
  /usr/bin/time -p "$RS" compile -nowarn -d "$OUT/scala-rs" "$DIR/$k"/*.scala > "$OUT/scala-rs.log" 2>&1
  rs_rc=$?
  st=$(wall "$OUT/scalac.log"); rt=$(wall "$OUT/scala-rs.log")
  if (( sc_rc != 0 || rs_rc != 0 )); then
    verdict="compile failed (scalac $sc_rc, scala-rs $rs_rc)"
  elif [[ "$(first_lines "$OUT/scalac")" == "$(first_lines "$OUT/scala-rs")" ]]; then
    verdict=same
  else
    verdict=DIFFERENT
  fi
  printf '| %-16s | %7.2fs | %7.2fs | %5.2f | %s |\n' "$k" "$st" "$rt" $(( rt / st )) "$verdict"
done
