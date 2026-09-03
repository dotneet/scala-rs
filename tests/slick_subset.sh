#!/bin/zsh
# The goal's second half is that slick *runs*. `slick_measure.sh` counts type
# errors and stops there (`classes=0` while any file fails). This script finds
# the fixpoint of files that compile *together* cleanly, emits their classes,
# and then actually loads every emitted class with the verifier on -- the
# first measurement on the "runs" axis.
set -e
SP=/private/tmp/claude-501/-Users-shinji-projects-scala-rs/0c32a046-384e-4a5f-9276-add7f58fd709/scratchpad/slick
SRC=$SP/slick/slick/src/main
COMPAT=$SP/slick/slick-compat-collections/src/main/scala-2.13+
ROOT=${ROOT:-$(cd "$(dirname $0)/.." && pwd)}
BIN=${SCALA_RS:-$ROOT/target/release/scala-rs}
RUN=$SP/subset-$$
GEN=$RUN/generated
rm -rf $RUN; mkdir -p $RUN
python3 "$ROOT/tests/expand_fm.py" $SRC/scala $GEN >/dev/null
REFLECT=/tmp/scala-2.13.16/lib/scala-reflect.jar
CP="$(cat $SP/deps.cp):$REFLECT"
LIB=/tmp/scala-rs-lib/scala-library-2.13.16.jar
find $SRC/scala $SRC/scala-2 $COMPAT $GEN -name '*.scala' | sort > $RUN/files.txt
# Round 1 compiles exactly what `slick_measure.sh` just compiled -- all 184
# files, same flags -- and that pass costs 4.5 minutes. When a fresh measure
# log is available, start from its verdict instead. `SLICK_SEED_LOG` is set by
# the verification pipeline; without it the loop runs from scratch as before.
if [[ -n ${SLICK_SEED_LOG:-} && -s ${SLICK_SEED_LOG:-/nonexistent} ]]; then
  if grep -q "panicked at" $SLICK_SEED_LOG; then
    echo "COMPILER PANIC in the seed measurement (not a fixpoint):" >&2
    grep -m1 "panicked at" $SLICK_SEED_LOG >&2
    exit 1
  fi
  # Only *errors* evict a file. A `-->` line also follows every **warning**,
  # and taking those too threw out `JdbcActionComponent.scala` on a clean
  # (0-error, 2-warning) measurement -- which then made the files that depend
  # on it fail, and the loop shrank a converged set from 184 to 132.
  grep -A 2 '^error' $SLICK_SEED_LOG | grep -oE '^\s+--> [^:]+\.scala' \
    | awk '{print $2}' | sort -u > $RUN/seed_bad.txt
  if [[ -s $RUN/seed_bad.txt ]]; then
    grep -vxF -f $RUN/seed_bad.txt $RUN/files.txt > $RUN/files2.txt
    mv $RUN/files2.txt $RUN/files.txt
  fi
fi
for round in 1 2 3 4 5 6 7 8; do
  OUT=$RUN/out; rm -rf $OUT; mkdir -p $OUT
  $BIN compile $(cat $RUN/files.txt) -d $OUT -cp "$CP" -Xsource:3 \
    --scala-library $LIB > $RUN/log.txt 2>&1 || true
  # Files named in any error line leave the set; repeat until none are.
  # A panic prints no `error:` lines and no `-->`, which used to read as
  # "converged, clean" -- round 2 once reported 126 files / 0 classes off a
  # crash in file one. A crash is a compiler bug, never a fixpoint.
  if grep -q "panicked at" $RUN/log.txt; then
    echo "COMPILER PANIC (not a fixpoint):" >&2
    grep -m1 "panicked at" $RUN/log.txt >&2
    exit 1
  fi
  grep -A 2 '^error' $RUN/log.txt | grep -oE '^\s+--> [^:]+\.scala' \
    | awk '{print $2}' | sort -u > $RUN/bad.txt
  if [[ ! -s $RUN/bad.txt ]]; then break; fi
  grep -vxF -f $RUN/bad.txt $RUN/files.txt > $RUN/files2.txt
  mv $RUN/files2.txt $RUN/files.txt
done
NFILES=$(wc -l < $RUN/files.txt | tr -d ' ')
NCLASSES=$(find $RUN/out -name '*.class' | wc -l | tr -d ' ')
# Load every class with verification on. Class.forName(initialize=false)
# still runs the bytecode verifier, without executing initializers.
cat > $RUN/V.java <<'JAVA'
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
(cd $RUN && javac V.java >/dev/null 2>&1)
java -Xverify:all -cp "$RUN:$RUN/out:$LIB:$CP" V $RUN/out $LIB 2>&1 | tail -5
echo "subset_files=$NFILES classes=$NCLASSES (of 184 sources)"
rm -rf $RUN
