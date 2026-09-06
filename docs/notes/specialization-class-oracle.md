# Class-owned specialization oracle

`tests/specialization_class_oracle.sh` is the small executable oracle for the
class-owned `@specialized(Int, Long)` ABI. It compiles
`tests/fixtures/specialization_class_oracle_provider.scala` with real scalac
2.13.16, compiles a fresh nsc client against the resulting classfiles, checks
the descriptors and bytecode with `javap`, and runs the client with
`java -Xverify:all`.

Run it with the JDK and Scala release used by the interoperability gates:

```sh
JAVA_HOME=/Library/Java/JavaVirtualMachines/temurin-17.jdk/Contents/Home \
PATH=/Library/Java/JavaVirtualMachines/temurin-17.jdk/Contents/Home/bin:$PATH \
tests/specialization_class_oracle.sh
```

The provider must emit these ten classfiles:

```text
OracleBox.class
OracleBox$mcI$sp.class
OracleBox$mcJ$sp.class
OracleIntBox.class
OracleLongBox.class
OracleStringBox.class
OracleReadable.class
OracleReadable$mcI$sp.class
OracleReadable$mcJ$sp.class
OracleReadableInt.class
```

The checks cover the generic `OracleBox<A>` field and fallback method, the
generic-owner primitive dispatch methods, primitive fields and constructors on
both siblings, boxed `Object` bridges, `specInstance$`, direct subclass parent
selection, and specialized trait default/static helpers. `OracleReadableInt`
also pins the primitive implementation and erased `Object` bridge for an
overridden trait member.

The nsc client must select `OracleBox$mcI$sp` and `OracleBox$mcJ$sp` for
primitive construction and getter/setter calls, keep `OracleBox` and its
`Object` methods for `String` and `fallback`, and dispatch the specialized
trait entry. Under scalac 2.13.16 the verified output is:

```text
3:5:su:25:47:sv:3:f
```

This is an nsc producer/client oracle. The current scala-rs stage 1 compiler
does not emit the `$mcI$sp`/`$mcJ$sp` class variants; its class ABI remains the
separate red implementation gate described in `docs/specialization.md`.
