#!/bin/zsh
# Compare scala-rs with scalac 2.13.16 on the workloads of
# tests/library_bench_gen.py: one fresh compile each against the libraries the
# kind's cp.txt names, then both programs' first lines of output.
#
# Usage: tests/library_bench.sh DIR KIND...
#   DIR holds the generated kinds (tests/library_bench_gen.py DIR).
#   SCALA_RS (default target/release/scala-rs), SCALAC and SCALA_LIBRARY_JAR
#   override the tools; the libraries come from the Coursier cache (COURSIER_CACHE
#   overrides its maven2 root). scalac gets a 2 GB heap, as in scalac_bench.sh.
set -u
ROOT=${0:A:h:h}
RS=${SCALA_RS:-$ROOT/target/release/scala-rs}
SC=${SCALAC:-/tmp/scala-2.13.16/bin/scalac}
LIB=${SCALA_LIBRARY_JAR:-/tmp/scala-rs-lib/scala-library-2.13.16.jar}
C=${COURSIER_CACHE:-$HOME/Library/Caches/Coursier/v1/https/repo1.maven.org/maven2}
typeset -A J
J[cats]="$C/org/typelevel/cats-core_2.13/2.13.0/cats-core_2.13-2.13.0.jar:$C/org/typelevel/cats-kernel_2.13/2.13.0/cats-kernel_2.13-2.13.0.jar"
J[ce]="$C/org/typelevel/cats-mtl_2.13/1.6.0/cats-mtl_2.13-1.6.0.jar:$C/org/typelevel/cats-effect_2.13/3.7.1/cats-effect_2.13-3.7.1.jar:$C/org/typelevel/cats-effect-kernel_2.13/3.7.1/cats-effect-kernel_2.13-3.7.1.jar:$C/org/typelevel/cats-effect-std_2.13/3.7.1/cats-effect-std_2.13-3.7.1.jar"
J[fs2]="$C/co/fs2/fs2-core_2.13/3.13.0/fs2-core_2.13-3.13.0.jar:$C/org/scodec/scodec-bits_2.13/1.2.4/scodec-bits_2.13-1.2.4.jar"
J[circe]="$C/io/circe/circe-core_2.13/0.14.7/circe-core_2.13-0.14.7.jar:$C/io/circe/circe-numbers_2.13/0.14.7/circe-numbers_2.13-0.14.7.jar"
J[xml]="$C/org/scala-lang/modules/scala-xml_2.13/2.4.0/scala-xml_2.13-2.4.0.jar"
DIR=$1; shift
wall() { awk '/^real/ { print $2 }' "$1" }
printf '| %-12s | %8s | %8s | %5s | %s |\n' kind scalac scala-rs ratio output
for k in "$@"; do
  D=$DIR/$k; CP=""
  for l in $(cat $D/cp.txt); do CP="$CP:${J[$l]}"; done
  CP=${CP#:}
  CPA=(); [[ -n $CP ]] && CPA=(-cp $CP)
  OUT=$DIR/out/$k
  rm -rf "$OUT"; mkdir -p "$OUT/scalac" "$OUT/scala-rs"
  /usr/bin/time -p env JAVA_OPTS="-Xmx2g -Xss8m" "$SC" -nowarn "${CPA[@]}" -d "$OUT/scalac" $D/*.scala > "$OUT/scalac.log" 2>&1
  sc_rc=$?
  /usr/bin/time -p "$RS" compile -nowarn "${CPA[@]}" --scala-library "$LIB" -d "$OUT/scala-rs" $D/*.scala > "$OUT/scala-rs.log" 2>&1
  rs_rc=$?
  st=$(wall "$OUT/scalac.log"); rt=$(wall "$OUT/scala-rs.log")
  if (( sc_rc != 0 || rs_rc != 0 )); then
    verdict="compile failed (scalac $sc_rc, scala-rs $rs_rc)"
  else
    run() { java -Xss8m -cp "$1:$LIB:$CP" Main 2>&1 | head -3 }
    if [[ "$(run "$OUT/scalac")" == "$(run "$OUT/scala-rs")" ]]; then verdict=same; else verdict=DIFFERENT; fi
  fi
  printf '| %-12s | %7.2fs | %7.2fs | %5.2f | %s |\n' "$k" "$st" "$rt" $(( rt / st )) "$verdict"
done
