# `@specialized`

The single largest cluster of corpus failures attributable to one missing
feature: **107 tests** (70 `pos`, 37 `run`) stop at
`unimplemented syntax: annotation specialized`, thrown by the parser. Every
other cluster in the corpus is either smaller than 40 or is many unrelated
roots wearing one diagnostic.

This document says what the 107 actually need, because they do not all need the
same thing, and the difference decides how the work is staged.

## What the 107 are

`@specialized` is a *performance* annotation. A program compiled with
specialization and one compiled without compute the same answers; what changes
is boxing and the set of classes on disk. So a test that merely *uses* the
annotation is not testing specialization, and does not need the phase to pass.
Splitting the 107 on that line:

| group | pos | run | total | what it needs |
| --- | ---: | ---: | ---: | --- |
| observes the specialized ABI — `$sp` method names, `Foo$mcI$sp` class names, `getClass.getName`, primitive `isInstanceOf` | 1 | 8 | **9** | the real phase |
| named `spec-*` — written to exercise specialization | 35 | 0 | **35** | the real phase, but `pos` only checks that it type-checks |
| uses the annotation incidentally; the test is about something else | 34 | 29 | **63** | acceptance only |

The 9 were found by grepping the sources for `$sp`, `mc[IJDFZBCSV]+$`,
`getClass.getName`, `classOf[`, and primitive `isInstanceOf`; the list is
reproducible from a corpus log and is recorded in the wave scratch directory.

## Why this is not `-no-specialization`

[`docs/scala-corpus.md`](scala-corpus.md#why-pos-does-not-pass--no-specialization)
explains why the corpus does not pass nsc's `-no-specialization`: the flag means
*ignore the annotation*, so passing it would score "we ignored what the test was
testing" as a pass, and it would hide the 44 tests that genuinely need the phase
along with the 63 that do not.

Accepting the annotation and **recording it on the symbol** is a different
thing. It is the first half of an implementation, not a way of not doing one —
provided the second half stays visibly undone. The next section is how to keep
it visible.

## Staging

### Stage 1 — accept and record

Parse `@specialized` and `@unspecialized`, including the argument list
(`@specialized(Int, Long)`, `@specialized(Specializable.Primitives)`), and
attach it to the type parameter's symbol. Emit nothing new. Type checking is
unaffected: nsc's `specialize` runs after the typer, and the typer applies no
rule that depends on the annotation.

Predicted effect: the **63 incidental tests go green, honestly** — the compiler
now accepts what the language accepts, and the programs mean the same thing
either way. The **35 `spec-*` `pos` tests also go green, and that pass is
weaker than nsc's**: they only assert that the program type-checks, and we
would be type-checking a program we then fail to specialize. The 9 stay red.

That prediction is worth stating precisely because it may be wrong. Several
past estimates on this project were wrong in both directions — 579 turned out
to be duplication, 220 turned out to be 1, 288 turned out to be 28. Measure it;
do not report the prediction.

### The ledger, so stage 1 cannot be mistaken for the whole job

Stage 1 raises the `pos` number by more than it earns. The fix is not to
suppress the number but to publish a second one beside it that stage 1 cannot
move: **compare the set of classfile names we emit against scalac's, for the 35
`spec-*` tests.**

That check is already one of this project's detection methods (see
[`docs/testing.md`](testing.md)), it is cheap, and it fails on exactly the thing
stage 1 does not do. Until `Foo$mcI$sp` appears in our output, the ledger says
so, whatever `pos` reports.

### Stage 2 — the phase

nsc's `specialize` is one of its most intricate phases. In outline:

* for `class C[@specialized T]`, generate a subclass `C$mcI$sp` per selected
  primitive, with `T` fixed and the members re-typed;
* for `def f[@specialized T](x: T)`, generate `f$mcI$sp` alongside, with the
  generic method forwarding;
* rewrite call sites and `new` to the specialized variant when the type
  argument is statically a primitive;
* honour the annotation's argument list, which restricts the primitive set, and
  `@unspecialized` on a member, which opts it out;
* handle specialized traits, where the specialized member lands on the
  interface with a default body.

We already *consume* specialized classes from the real library jar —
`crates/typer/src/pickle_supply.rs` has `despecialized()`, which maps
`Foo$mcI$sp` back to `Foo` so a pickled parent matches. That is the reading
side, and it stays; stage 2 is the writing side.

The `spec-*` tests are the specification: 35 programs someone else wrote to
pin down this phase's behaviour, with scalac's output to diff against.

## Order

Stage 1 is small, self-contained, touches the parser and the symbol table, and
unblocks 63 tests that have nothing to do with specialization — including 29
`run` tests whose real subject is something else we might be getting wrong.
Getting them running is worth more than the specialization work itself in the
short term, because a `run` test that cannot compile is a defect we cannot see.

Stage 2 is a phase, and should be planned as one.
