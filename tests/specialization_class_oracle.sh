#!/bin/zsh
# Verify the class-owned @specialized(Int, Long) ABI emitted by scalac 2.13.16.
#
# This is an oracle for the future scala-rs class phase.  It deliberately
# compiles the provider and consumer with real scalac, then checks class names,
# descriptors, bridge bytecode, nsc call-site owners, and JVM output.  It does
# not call scala-rs: the existing compiler is expected to remain red on this
# ABI until class specialization is implemented.
set -euo pipefail

ROOT=${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}
SCALAC=${SCALAC:-/tmp/scala-2.13.16/bin/scalac}
LIB=${SCALA_LIBRARY:-/tmp/scala-rs-lib/scala-library-2.13.16.jar}

if [[ -n ${JAVA_HOME:-} && -x "$JAVA_HOME/bin/java" && -x "$JAVA_HOME/bin/javac" && -x "$JAVA_HOME/bin/javap" ]]; then
  JAVA_HOME=${JAVA_HOME:A}
  JAVA="$JAVA_HOME/bin/java"
  JAVAC="$JAVA_HOME/bin/javac"
  JAVAP="$JAVA_HOME/bin/javap"
  PATH="$JAVA_HOME/bin:$PATH"
  JAVACMD=$JAVA
  export JAVA_HOME PATH JAVACMD
else
  JAVA=$(command -v java || true)
  JAVAC=$(command -v javac || true)
  JAVAP=$(command -v javap || true)
  if [[ -z $JAVA || -z $JAVAC || -z $JAVAP ]]; then
    echo "specialization class oracle: Java java/javac/javap unavailable" >&2
    exit 2
  fi
  unset JAVA_HOME
  JAVACMD=$JAVA
  export JAVACMD
fi

if [[ ! -x $SCALAC ]]; then
  SCALAC=$(command -v scalac || true)
fi
if [[ -z $SCALAC || ! -x $SCALAC || ! -f $LIB ]]; then
  echo "specialization class oracle: scalac or scala-library 2.13.16 unavailable" >&2
  exit 2
fi
if [[ "$($SCALAC -version 2>&1)" != *"Scala compiler version 2.13.16"* ]]; then
  echo "specialization class oracle: expected scalac 2.13.16 at $SCALAC" >&2
  "$SCALAC" -version 2>&1 || true
  exit 2
fi

WORK=${TMPDIR:-/tmp}/scala-rs-specialization-class-oracle-$$
mkdir -p "$WORK/provider" "$WORK/client"
trap 'rm -rf "$WORK"' EXIT

PROVIDER=$ROOT/tests/fixtures/specialization_class_oracle_provider.scala
CLIENT=$ROOT/tests/fixtures/specialization_class_oracle_client.scala

require_file() {
  [[ -f $1 ]] || { echo "missing file: $1" >&2; exit 1; }
}

require_text() {
  local needle=$1
  local file=$2
  grep -Fq -- "$needle" "$file" || {
    echo "missing ${(q)needle} in $file" >&2
    exit 1
  }
}

require_file "$PROVIDER"
require_file "$CLIENT"

"$SCALAC" -Xno-forwarders -classpath "$LIB" -d "$WORK/provider" "$PROVIDER"

for class in \
  OracleBox.class \
  'OracleBox$mcI$sp.class' \
  'OracleBox$mcJ$sp.class' \
  OracleIntBox.class \
  OracleLongBox.class \
  OracleStringBox.class \
  OracleReadable.class \
  'OracleReadable$mcI$sp.class' \
  'OracleReadable$mcJ$sp.class' \
  OracleReadableInt.class; do
  require_file "$WORK/provider/$class"
done

"$JAVAP" -classpath "$WORK/provider" -p -s \
  OracleBox 'OracleBox$mcI$sp' 'OracleBox$mcJ$sp' \
  OracleIntBox OracleLongBox OracleStringBox \
  OracleReadable 'OracleReadable$mcI$sp' 'OracleReadable$mcJ$sp' \
  OracleReadableInt > "$WORK/provider-signatures.txt"
"$JAVAP" -classpath "$WORK/provider" -p -c -s \
  'OracleBox$mcI$sp' 'OracleBox$mcJ$sp' OracleIntBox OracleLongBox \
  OracleStringBox OracleReadable OracleReadableInt > "$WORK/provider-bytecode.txt"

# Generic owner: source-shaped storage, fallback method, primitive dispatch,
# and the marker method that distinguishes a specialized runtime instance.
require_text 'public class OracleBox<A>' "$WORK/provider-signatures.txt"
require_text 'public A value;' "$WORK/provider-signatures.txt"
require_text 'descriptor: Ljava/lang/Object;' "$WORK/provider-signatures.txt"
require_text 'public <B> B fallback(B);' "$WORK/provider-signatures.txt"
require_text 'public int value$mcI$sp();' "$WORK/provider-signatures.txt"
require_text 'descriptor: ()I' "$WORK/provider-signatures.txt"
require_text 'public long value$mcJ$sp();' "$WORK/provider-signatures.txt"
require_text 'descriptor: ()J' "$WORK/provider-signatures.txt"
require_text 'public void value$mcI$sp_$eq(int);' "$WORK/provider-signatures.txt"
require_text 'descriptor: (I)V' "$WORK/provider-signatures.txt"
require_text 'public void value$mcJ$sp_$eq(long);' "$WORK/provider-signatures.txt"
require_text 'descriptor: (J)V' "$WORK/provider-signatures.txt"
require_text 'public int get$mcI$sp();' "$WORK/provider-signatures.txt"
require_text 'public long get$mcJ$sp();' "$WORK/provider-signatures.txt"
require_text 'public void set$mcI$sp(int);' "$WORK/provider-signatures.txt"
require_text 'public void set$mcJ$sp(long);' "$WORK/provider-signatures.txt"
require_text 'public boolean specInstance$();' "$WORK/provider-signatures.txt"
require_text 'public OracleBox(A);' "$WORK/provider-signatures.txt"
require_text 'descriptor: (Ljava/lang/Object;)V' "$WORK/provider-signatures.txt"

# Specialized siblings: primitive fields and constructors, primitive entries,
# and erased Object bridges all exist on each selected variant.
require_text 'public class OracleBox$mcI$sp extends OracleBox<java.lang.Object>' "$WORK/provider-signatures.txt"
require_text 'public int value$mcI$sp;' "$WORK/provider-signatures.txt"
require_text 'public OracleBox$mcI$sp(int);' "$WORK/provider-signatures.txt"
require_text 'public int get();' "$WORK/provider-signatures.txt"
require_text 'public void set(int);' "$WORK/provider-signatures.txt"
require_text 'public java.lang.Object get();' "$WORK/provider-signatures.txt"
require_text 'public void set(java.lang.Object);' "$WORK/provider-signatures.txt"
require_text 'public class OracleBox$mcJ$sp extends OracleBox<java.lang.Object>' "$WORK/provider-signatures.txt"
require_text 'public long value$mcJ$sp;' "$WORK/provider-signatures.txt"
require_text 'public OracleBox$mcJ$sp(long);' "$WORK/provider-signatures.txt"
require_text 'public long get();' "$WORK/provider-signatures.txt"
require_text 'public void set(long);' "$WORK/provider-signatures.txt"
require_text 'public java.lang.Object value();' "$WORK/provider-signatures.txt"
require_text 'public void value_$eq(java.lang.Object);' "$WORK/provider-signatures.txt"
require_text 'ireturn' "$WORK/provider-bytecode.txt"
require_text 'lreturn' "$WORK/provider-bytecode.txt"
require_text 'boxToInteger' "$WORK/provider-bytecode.txt"
require_text 'boxToLong' "$WORK/provider-bytecode.txt"
require_text 'unboxToInt' "$WORK/provider-bytecode.txt"
require_text 'unboxToLong' "$WORK/provider-bytecode.txt"

# Direct subclass construction selects the corresponding parent variant;
# reference construction retains the generic parent constructor.
require_text 'public class OracleIntBox extends OracleBox$mcI$sp' "$WORK/provider-signatures.txt"
require_text 'public class OracleLongBox extends OracleBox$mcJ$sp' "$WORK/provider-signatures.txt"
require_text 'public class OracleStringBox extends OracleBox<java.lang.String>' "$WORK/provider-signatures.txt"
require_text 'public int get();' "$WORK/provider-signatures.txt"
require_text 'public long get();' "$WORK/provider-signatures.txt"
require_text 'public java.lang.Object get();' "$WORK/provider-signatures.txt"
require_text 'Method OracleBox$mcI$sp."<init>":(I)V' "$WORK/provider-bytecode.txt"
require_text 'Method OracleBox$mcJ$sp."<init>":(J)V' "$WORK/provider-bytecode.txt"
require_text 'Method OracleBox."<init>":(Ljava/lang/Object;)V' "$WORK/provider-bytecode.txt"
require_text 'Method value$mcI$sp:()I' "$WORK/provider-bytecode.txt"
require_text 'Method value$mcJ$sp:()J' "$WORK/provider-bytecode.txt"

# Specialized trait and implementation bridges are part of this ABI oracle.
require_text 'public interface OracleReadable<A>' "$WORK/provider-signatures.txt"
require_text 'public static int read$mcI$sp$(OracleReadable);' "$WORK/provider-signatures.txt"
require_text 'public default int read$mcI$sp();' "$WORK/provider-signatures.txt"
require_text 'public static long read$mcJ$sp$(OracleReadable);' "$WORK/provider-signatures.txt"
require_text 'public default long read$mcJ$sp();' "$WORK/provider-signatures.txt"
require_text 'public interface OracleReadable$mcI$sp extends OracleReadable<java.lang.Object>' "$WORK/provider-signatures.txt"
require_text 'public interface OracleReadable$mcJ$sp extends OracleReadable<java.lang.Object>' "$WORK/provider-signatures.txt"
require_text 'public class OracleReadableInt extends OracleBox$mcI$sp implements OracleReadable$mcI$sp' "$WORK/provider-signatures.txt"
require_text 'public int read();' "$WORK/provider-signatures.txt"
require_text 'public int read$mcI$sp();' "$WORK/provider-signatures.txt"
require_text 'public long read$mcJ$sp();' "$WORK/provider-signatures.txt"
require_text 'public java.lang.Object read();' "$WORK/provider-signatures.txt"

# Compile a fresh nsc client against only the provider output and the real
# library. Its bytecode must select primitive owners for Int/Long, generic
# Object entries for String/fallback, and the specialized trait dispatch.
"$SCALAC" -Xno-forwarders \
  -classpath "$WORK/provider:$LIB" -d "$WORK/client" "$CLIENT"
"$JAVAP" -classpath "$WORK/client:$WORK/provider:$LIB" -p -c \
  'OracleClassClient$' > "$WORK/client-bytecode.txt"
require_text 'class OracleBox$mcI$sp' "$WORK/client-bytecode.txt"
require_text 'Method OracleBox$mcI$sp."<init>":(I)V' "$WORK/client-bytecode.txt"
require_text 'class OracleBox$mcJ$sp' "$WORK/client-bytecode.txt"
require_text 'Method OracleBox$mcJ$sp."<init>":(J)V' "$WORK/client-bytecode.txt"
require_text 'class OracleBox' "$WORK/client-bytecode.txt"
require_text 'Method OracleBox."<init>":(Ljava/lang/Object;)V' "$WORK/client-bytecode.txt"
require_text 'Method OracleBox.get$mcI$sp:()I' "$WORK/client-bytecode.txt"
require_text 'Method OracleBox.set$mcI$sp:(I)V' "$WORK/client-bytecode.txt"
require_text 'Method OracleBox.get$mcJ$sp:()J' "$WORK/client-bytecode.txt"
require_text 'Method OracleBox.set$mcJ$sp:(J)V' "$WORK/client-bytecode.txt"
require_text 'Method OracleBox.get:()Ljava/lang/Object;' "$WORK/client-bytecode.txt"
require_text 'Method OracleBox.set:(Ljava/lang/Object;)V' "$WORK/client-bytecode.txt"
require_text 'Method OracleBox.fallback:(Ljava/lang/Object;)Ljava/lang/Object;' "$WORK/client-bytecode.txt"
require_text 'InterfaceMethod OracleReadable.read$mcI$sp:()I' "$WORK/client-bytecode.txt"

got=$("$JAVA" -Xverify:all -cp "$WORK/client:$WORK/provider:$LIB" OracleClassClient)
expected='3:5:su:25:47:sv:3:f'
[[ $got == "$expected" ]] || {
  echo "unexpected JVM output: ${(q)got} (expected ${(q)expected})" >&2
  exit 1
}

echo "specialization class oracle: PASS"
echo "scalac: $SCALAC"
echo "java: $JAVA"
echo "javac: $JAVAC"
echo "javap: $JAVAP"
echo "provider: $WORK/provider (cleaned on exit)"
echo "JVM output: $got"
