# Codegen Regression Repairs from Candidate 81454c19 (Not Accepted)

The starting point was `81454c19`. The ClassProjection work in inner classes is
not included.

## Performance regression from repeated typing

The candidate workspace run finished with 2275 PASS / 0 FAIL (205 result rows,
1118.45 seconds). Slick reported 0 errors across 1490 classes, and Cats
reported 355 errors across 83 files. The GitBucket compiler then continued at
about 98% CPU. The parent sampled its stack for one second and sent SIGTERM to
that compiler process at 10 minutes 51 seconds. The validation runner recorded
GitBucket exit 2 and stopped with exit 1.

**GitBucket was incomplete for this candidate. The same run did not perform the
remaining Scala-library measure, strict verification, Slick execution,
specialization ledger, or corpus. It must not be treated as a restarted run.**

The issue reproduced with `object Main { val x = missing.f(0).f(0)... }`:
12 nesting levels took 0.79 seconds, while 16 and 20 levels each timed out at
five seconds. `try_rewrite_dynamic_apply` typechecked a qualifier that already
had a type. The ordinary selection path also retried a provisional `Error`, so
each nesting level checked the same tree again. The dynamic probe now types only
a `NoType` qualifier and reuses a completed type otherwise; the ordinary
provisional-`Error` retry remains.

After the repair, 20 levels took 0.10 seconds, 40 took 0.17 seconds, and 80
took 0.46 seconds. A real GitBucket measure finished in 10.93 seconds with
906 errors across 112 files. A lower diagnostic count is not sufficient: new
differences such as constructor ambiguity were not audited, so the candidate
did not meet the main baseline of 912 / 111.

The persistent test
`qualifier_retry::erroneous_application_chain_does_not_repeat_dynamic_receiver_typing`
requires the 80-level invalid expression to be rejected within 20 seconds and
to report one undefined-name diagnostic. The generous limit detects the old
explosive rechecking within a finite time; it is not a normal-case performance
target. Existing cross-unit valid/invalid, file-order, and nsc/scala-rs
comparisons also pass.

## Typed patterns for Function subclasses

`A => B` and `Function1[A, B]` had separate representations and erased
abstract type arguments with different rules. The type-pattern compatibility
entry point now normalizes function syntax to a `FunctionN` class and applies
the same argument rules to both forms. Four false Cats diagnostics disappeared.

`function_subclass_pattern.scala` executes Constant, Wrapped, Zero, and
wildcard patterns and compares them with nsc. An invalid final subclass with a
String result is also checked against an expected Int result for Function1 and
Function0; both compilers reject it.

## Verified results and remaining work

- Cargo: function_pattern 1, qualifier_retry 2, seqpat 20, dynamic e2e 5;
  28 tests passed.
- Fresh release build and `git diff --check` succeeded.
- Slick: 184 files / 0 errors / 1490 classes (2.26 seconds).
- Cats: 339 / skip1 / 351 errors / 81 files (2.29 seconds).
- GitBucket: 353 / skip1 / 906 errors / 112 files (10.93 seconds).
- Scala library: 538 / 1620 errors / 171 files (2.01 seconds).

One new Cats `monadError` `andThen` ambiguity remained. The Scala library had
additional diagnostics in TNode/CNode/LNode, MutableBufferWrapper, and List
type patterns, while FileProp constructor ambiguity disappeared. Investigate
the actual symbols and inheritance relationships before deciding that missing
type information makes a candidate inapplicable. Do not widen types to `Any`
or disable type-pattern checks merely to match counts.

The new full gate must wait until those questions are resolved. This candidate
is not mergeable.

Evidence:

- `/tmp/scala-rs-codex/integration/candidate-81454c1/results.json`
- The accompanying `gitbucket-sample.txt`, `gitbucket-parent-interruption.json`,
  and `cats-parent-diff.txt`
- `/tmp/scala-rs-codex/integration/error-retry-growth/results.json`
- The accompanying `reused-qualifier/results.json`, `focused.log`, and
  `dynamic-focused.log`
- The accompanying `measures/results.json` and each `*-baseline-diff.txt`

## Removing forced List/Option/Some resolution

`check_types::tree_to_type(AppliedTypeTree)` returned a prelude symbol whenever
the final name was List, Option, or Some, regardless of its qualifier or source
definition. A comment explicitly kept this behavior because resolving source
definitions increased the library error count. That was not compatible,
however: `custom.List[Int]` also became scala's List. All three names now use
ordinary constructor lookup and type application.

The old fixture could not find the constructors or `value` accessors for
`custom.List` and `custom.Option`. The repaired fixture combines qualified type
annotations, imported constructors, and an explicit scala List and prints
`7/option/9/3`, matching nsc. Invalid assignments from
`custom.List[Int]` to `custom.List[String]` and from
`custom.Option[Int]` to `custom.Option[String]` are rejected by both compilers.
Strict JVM execution also succeeds.

The related 24 tests passed: applied_collection_names 1, aliaslookup 2,
function_pattern 1, and seqpat 20. The applied_collection_names test was run
again after adding the imported-constructor case. No type-pattern check was
disabled and no type was replaced with `Any`.

Additional measurements with a fresh release build, JDK 17, and UTF-8 were:

- Cats: 351 errors / 81 files (1.85 seconds)
- GitBucket: 902 errors / 112 files (5.52 seconds)
- Slick: 0 errors / 1490 classes (1.76 seconds)
- Scala library: **1880 errors / 203 files** (1.55 seconds; previously 1620 / 171)

The additional diagnostics were not all audited. Do not restore an incorrect
symbol resolution solely because the count worsened, but do not assume every
new diagnostic is valid either. The full workspace and corpus must be checked,
and any real compatibility regressions repaired. This remained a candidate
before full acceptance, so the main baseline was not updated.

Additional evidence is in `/tmp/scala-rs-codex/integration/applied-collection-names/`:
`before.log`, `focused.log`, `import-focused.log`, the `measures/` logs, and the
previous diff.

### Separated missing information

A temporary type-relation trace was collected and all debug code was removed.
The `PATREL` lines in `error-retry-growth/pattern-trace.log` recorded:

- TNode and related scrutinees remained
  `Named { name: "MainNode", args: [] }`, even though `class_sym_of` found the
  actual MainNode symbol in the same context.
- The MutableBufferWrapper scrutinee `java.util.List` became the scala
  immutable List prelude symbol #50. The source argument was written as
  `ju.List[A]`, so this matched the removed AppliedTypeTree path that ignored
  qualifiers. The false MutableBufferWrapper diagnostic disappeared after the
  repair.
- A separate issue was found in the classpath descriptor reader, which favored
  a simple name over the full JVM name. This was not established as the cause
  of the current diagnostic; reproduce it separately with same-named Java
  classes in provider and consumer runs.
- Source List type patterns also used prelude #50 with `parents=[AnyRef]`.
  This path and the custom fixture directly motivated the name-resolution
  change.

The trace was collected without an explicitly fixed JDK environment, so it is
not measurement evidence. Use the fixed-environment measurements above for
numeric comparisons.
