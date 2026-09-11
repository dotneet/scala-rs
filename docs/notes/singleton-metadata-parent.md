# Singleton Separate Compilation and Macro Receivers

`Singleton` erases to `Object` on the JVM, but ScalaSignature must retain it as
`scala.Singleton`. The reader restores the built-in symbol, and the writer must
not synthesize `java.lang.Singleton` from the JVM name.

The abbreviated method information created by the initial classpath scan has
no type-parameter bounds. When such a class is used, the existing complete
pickle load refreshes it. The same refresh happens when inherited members are
entered into a scope, and stale method symbols are removed. Source definitions
and the handwritten prelude are excluded from this replacement.

The macro's `c.universe.WeakTypeTag` is obtained from an internal object by
evaluating its receiver. When an explicit qualifier returns the target object,
code generation uses that expression and does not replace it with a static
`MODULE$` read merely because an external class has incomplete owner metadata.

The `singleton_metadata` test covers all four provider/consumer combinations
compiled by scalac 2.13.16 and scala-rs. A valid override runs under
`java -Xverify:all` and prints `7` and `bound`; an override with an illegally
narrowed bound is rejected. Existing engine tests cover macro expansion and
execution.

Evidence is under `/tmp/scala-rs-codex/integration/reify-audit-e521/`.
`bounds-roundtrip.log` records success for all four combinations. Every valid
case was executed, and invalid cases were checked by exit status and the
override diagnostic. The 27 engine tests and 18 Singleton cases in
`roundtrip-focused.log` passed, but the metadata tests in that same log failed
before the bounds-specific writer path was repaired; they must not be counted
as successful evidence. The four library measurements in
`roundtrip-measures/measures` used the same diagnostic counts as the earlier
candidate and also predate the final writer repair. This note does not mean
that the full validation gate or acceptance into `main` is complete.
