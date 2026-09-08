# Direct self-tail calls

Direct self-tail calls in `final` / `private` methods, object methods, and lifted
local defs are converted into JVM backward branches. An `@tailrec` annotation
uses the existing type-checker's effective-finality decision, including sealed
classes. Mutual recursion is outside the scope.

## Semantics

`gen_tailrec.rs` selects self calls in `if` branches, each `match` body, the last
expression in a block, typed expressions after erasure, and the **right operand
of `scala.Boolean.&&` and `scala.Boolean.||`**. Calls in arguments, conditions,
guards, `val` right-hand sides, nested definitions, and `try`/`finally` bodies
are not in tail position.

## `&&` and `||`

nsc's `TailCalls` special-cases two symbols, `Boolean_and` and `Boolean_or`, and
transforms their argument *in the tail context*. Both compile to a conditional
branch over the operand rather than to a call, so nothing in the method runs
after it. Seven `@tailrec` methods of the 2.13.16 library are written this way
and are rejected without the rule: `LinearSeq.sameElements`, `List.equals`,
`ListSet.containsInternal`, `StringParsers.forAllBetween`,
`Promise.tryComplete0`, `ClassManifestDeprecatedApis.subtype` and
`sys.process.Parser.skipToDelim`.

The test is `Intrinsic::BoolBin("&&") | BoolBin("||")`, which is installed only
on `scala.Boolean`. A user-defined `&&` or `||` on any other type is an ordinary
strict method, so its argument stays an argument position — that is nsc's rule
too, and `trc_bool_bad.scala` pins it.

`gen_bool_and` and `gen_bool_or` emit the left operand, branch on it, and `pop`
it before the right operand, so the right operand begins at the operand-stack
depth of the whole expression and the back edge is taken with the stack as it is
at the method's entry. The backend records the *application*, not the operand:
the ordinary `gen_apply` path reaches an argument only after
`flatten_apply_owned` has cloned it, so the tree `gen_expr` receives is at a
different address from the one `collect` scanned and the recorded pointer would
never match. `emit_tail_call` therefore emits the short circuit itself, from the
original subtrees.

Code generation evaluates the receiver and all arguments from left to right,
then stores them in reverse order in the JVM argument slots. This handles
swapped arguments, two-slot `Long`/`Double` values, `Unit`, boxing and unboxing
introduced by erasure, and lifted captured arguments. For a call with a different
receiver, slot 0 is updated after the arguments have been evaluated.
As with scalac's transformation, no extra null check is inserted.
`TrcNull.hop(2, null)` terminates because the body does not read a field while
the receiver is null and returns to the original instance on the next iteration.
This is the observed nsc behavior and differs from a null receiver in an ordinary
`invokevirtual`. The loop target precedes captured-field loads, so captured values
are refreshed after the receiver changes.

When a by-name parameter is passed unchanged to another by-name argument, the
typer forwards the existing thunk. Wrapping it as `() => x` on every iteration
would make the final value evaluation recurse through the thunk chain even after
the method body had become a loop.

An annotated method is rejected when an unsupported erased shape or an unhandled
tail call remains. The current type checker rejects self recursion in an explicit
`return`, and recursion inside `try`/`catch`/`finally`. These forms are not
claimed to be Scala 2.13 compatible. Value class `$extension` methods are
supported: their underlying receiver remains in slot 0 while the recursive
parameters are updated in place.

## Regression test

```sh
CARGO_BUILD_JOBS=2 RUST_TEST_THREADS=2 cargo test -p scala-rs-cli --release --test trc_tailrec
```

`tests/fixtures/trc_deep.scala` runs one to two million recursive calls with
`-Xss256k -Xverify:all` and checks output against scalac 2.13.16. It covers wide
argument swaps, `match` and blocks, local defs, mutable captures, receiver swaps,
argument side-effect order, curried and generic calls, `Unit`, by-name arguments,
unannotated final methods, and parameterless methods. `javap -p -c` also checks
that recursive calls disappear from each target method and that a branch is
generated.

`trc_client.scala` is an interoperability test in which scalac compiles a second
program against scala-rs classfiles. `trc_bad.scala` and `trc_inputs_bad.scala`
check that both compilers reject overridable methods, non-tail recursion, calls in
the receiver, and calls in an earlier argument clause. `trc_valueclass.scala`
runs primitive, wide, and reference underlying receivers through two million
calls, compares output with scalac, and checks the emitted `$extension`
descriptors and loop branches. `trc_valueclass_client.scala` is compiled by
scalac against scala-rs classfiles to exercise the static extension ABI across a
compilation boundary.

```sh
cargo test -p scala-rs-cli --release --test trc_bool
```

`tests/fixtures/trc_bool.scala` reproduces all seven library `&&` / `||` shapes
— including one that changes the receiver on every iteration and one that
stores two-slot `Long` arguments on the back edge — runs the deep ones two
million times under `-Xss256k`, and compares the output with scalac 2.13.16
compiling the same source. `javap -c` then checks that no self-`invoke` is left
in any of the eight methods and that each has a backward branch.
`trc_bool_bad.scala` holds the six shapes that must stay rejected: the *left*
operand of a short circuit, an operand consumed by `!`, a short circuit that is
not itself in tail position, a user-defined `||`, a call in a `val`'s
right-hand side, and an overridable method whose recursion is reached through
`||`. scalac rejects the same six.

## A JIT comparison trap with Zulu 15.0.6

The default Java in this development environment was the following version.

```
openjdk version "15.0.6" 2022-01-18
OpenJDK Runtime Environment Zulu15.38+17-CA (build 15.0.6+5-MTS)
OpenJDK 64-Bit Server VM Zulu15.38+17-CA (build 15.0.6+5-MTS, mixed mode)
```

`TrcDeep.matching(2000000, 0)` should return 2000000, but this VM's default JIT
returned a different small value on each run. **The same happened with the
program emitted by scalac 2.13.16.** Do not infer an invalid compiler transform
from scala-rs output alone.

```sh
# <out> is the directory where scala-rs or scalac compiled trc_deep.scala
java -Xverify:all -Xss256k -cp <out>:/tmp/scala-rs-lib/scala-library-2.13.16.jar TrcDeep
# The same classfiles return the expected value in these modes.
java -Xint -Xverify:all -Xss256k -cp <out>:/tmp/scala-rs-lib/scala-library-2.13.16.jar TrcDeep
java -XX:TieredStopAtLevel=1 -Xss256k -cp <out>:/tmp/scala-rs-lib/scala-library-2.13.16.jar TrcDeep
```

The default JIT of Temurin 17.0.3 was also checked with output from both
compilers, and both ran correctly. Regression tests prefer Temurin 17 when it is
available and use Java's `-Xint` otherwise. This does not claim to identify the
JIT's internal cause.
