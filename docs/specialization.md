# `@specialized`

**Status: method-owned Int/Long specialization is implemented; class-owned
specialization remains incomplete.** One method-owned type parameter is
specialized for object methods and final/private class methods, with primitive
entry points and call selection. Local classes, constructors and type symbols
are cloned for each entry. See [the implementation plan](specialization-plan.md)
for the current boundary and tests. No `Foo$mcI$sp` class is emitted on main;
`tests/spec_classfiles.sh` still records the class-specialization gap.

This used to be the single largest cluster of corpus failures attributable to
one missing feature: **107 tests** (70 `pos`, 37 `run` — measured later as 111:
70 `pos`, 33 `run`, 8 `neg`) stopped at
`unimplemented syntax: annotation specialized`, thrown by the parser. Every
other cluster in the corpus is either smaller than 40 or is many unrelated
roots wearing one diagnostic.

This document says what those tests actually need, because they do not all need
the same thing, and the difference decides how the work is staged.

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

### Stage 1 — accept and record — **done**

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

#### What it actually moved

Measured on the full corpus (`CORPUS_SIZE=full`), the same tree before and
after the change:

| | before | after | delta |
|---|---:|---:|---:|
| `pos` pass | 983 | 1042 | **+59** |
| `neg` pass | 653 | 647 | **−6** |
| `run` pass | 490 | 507 | **+17** |

First, the population. **111** tests stopped at
`unimplemented syntax: annotation specialized`, not 107, and they split
70 `pos` / 33 `run` / **8 `neg`** — the table at the top of this document
counted 37 `run` and no `neg` at all, because it was built from the `pos` and
`run` logs only.

Then the yield. Of the 111, **78 turned green** (59 `pos`, 17 `run`, and 2 of
the `neg` ones which had a second error anyway). The other 33 hit the next wall
immediately: nine `run` tests now compile and produce the wrong output, six
`neg` tests are now accepted when they should not be, and the rest stop at
`generic Array construction without ClassTag`, `Manifest[Int]`,
`method fromSpecific overrides nothing`, and a dozen one-offs. That is real
progress — a diagnostic about the actual subject beats a diagnostic about the
annotation — but it is 78 tests, not 111, and it is worth saying which.

#### `neg` went **down** by six, and five of the six are honest

Accepting more syntax cannot make us reject more programs, so a `neg` fall was
possible from the start. All six had been "passing" only because the annotation
was refused — they are among the 98 `neg` tests `docs/scala-corpus.md` counts
as rejected by a parse error or an "unimplemented" refusal rather than by the
check the test exists for.

Five of them are tests whose expected error **is produced by the specialize
phase**, so they are exactly the tests stage 2 owes:

| test | what scalac reports |
|---|---|
| `spec-overrides` | "Type parameter has to be specialized at least for the same types as in the overridden method" |
| `t4417` | protected constructor of `Pixel$mcD$sp` not accessible |
| `t4541` | protected `data` not accessible from `Sparse$mcI$sp` |
| `t5564` | bounds violated by the type arguments the phase infers for the forwarder |
| `t9014` | `Inner is already defined` — the phase duplicating a local case class |

The sixth, **`valueclasses`, is not about specialization at all**, and it is
the useful find. It is 30 lines of value-class violations — a `trait` extending
`AnyVal`, a nested value class, a local one, two constructor parameters, a
`var` parameter, a field in the body — and we now compile all of it and emit 33
classfiles. None of those rules is implemented. The refusal on line 30's
`@specialized` had been standing in for all of them, which is precisely the
failure mode "a rejection for the wrong reason" describes. This is a
pre-existing gap that this change uncovered, not one it caused.

#### What it did not move, and why that is the interesting part

The four compile measures (`slick`, `cats`, `scalalib`, `gitbucket`) all pass
nsc's `-no-specialization`, and all four report numbers identical to before —
`slick errors=0 classes=1596`, `cats errors=907`, `scalalib errors=1644`,
`gitbucket errors=1193`. That is by construction: under the flag the parser
still drops the annotation, exactly as it did.

What *has* changed is what those measures would report **without** the flag:

| without `-no-specialization` | before | after |
|---|---:|---:|
| `cats` (339 files) | 71 | **907** |
| `src/library` (538 files) | 84 | **1644** |

Both "before" figures were parse aborts, not type-checking figures — the run
stopped at the first `@specialized` and typechecked nothing (see the note at
the top of `docs/cats.md`). Both "after" figures are *identical to the numbers
those measures report with the flag*. The flag is now buying nothing, and the
ABI caveat it costs — `docs/scala-corpus.md` calls it "the right trade for a
progress measure and the wrong one for a conformance score" — is no longer
being paid for anything. Dropping `-no-specialization` from
`tests/cats_measure.sh` and `tests/scalalib_measure.sh` would leave every
number where it is today and remove the caveat. It is left in place here only
because changing a shared measure is not this slice's call.

#### Where stage 1 lives

* `crates/parser/src/specialization.rs` — reads an annotation tree into a
  `SpecializedTypes` set, following nsc's `SpecializeTypes.specializedOn`:
  no arguments means `Specializable.Primitives` (the nine primitive value
  classes, *not* `AnyRef`), a group name expands to its members exactly as
  `scala/Specializable.scala` spells them, anything else is a type name, and a
  name that is neither selects nothing. It also carries `SpecializedType::tag`,
  the `$mc<letter>$sp` letters stage 2 will build names from, so the two tables
  cannot drift apart.
* `crates/parser/src/parse.rs` — `parse_annotation` keeps the annotation and
  normalises the spelling: `import scala.{specialized => sp}` is tracked, and
  the head of the annotation tree is rewritten to `specialized`. nsc resolves
  the name to a symbol and every later phase sees `scala.specialized`; we have
  no symbol on an annotation, so the parser normalises instead. That is safe
  only because the annotation is inert — never type-checked, never pickled,
  never emitted.
* `crates/typer/src/symbol.rs` — `Symbol::specialized` (on the type parameter)
  and `Symbol::unspecialized` (on the member), filled by
  `SymbolTable::record_specialization`, called from `Typer::enter_tparams` and
  from the `DefDef` case of the namer.
* `crates/backend/src/pickle.rs` — `pickle_symannot` deliberately drops both
  annotations. Pickling `@specialized` would tell scalac that a class of ours
  is specialized and let it link to `$mc*$sp` members that do not exist.
* Tests: `crates/parser/src/specialization.rs` (readings, and that
  `-no-specialization` drops the annotation), `crates/typer/tests/specialization.rs`
  (what lands on the symbol; that the annotation does not soften type
  checking), `crates/cli/tests/e2e.rs` fixtures `sp_annot`, `sp_alias`,
  `sp_annot_bad`, `scalalib_spec`.

#### Known gap: the private runtime has no `scala.specialized`

The annotation itself never needs a symbol — nothing resolves it — so
`@specialized(Int) T` is accepted in both library modes. The *import* is a
different matter: `import scala.{specialized => sp}` and
`import scala.Specializable._` name terms that the private runtime
(`--no-scala-library`) does not define, so those two lines are an error there
and compile only against the real jar. This is why `tests/fixtures/sp_alias.scala`
is a library-mode fixture and `tests/fixtures/sp_annot.scala` is not.

### The ledger, so stage 1 cannot be mistaken for the whole job

Stage 1 raises the `pos` number by more than it earns. The fix is not to
suppress the number but to publish a second one beside it that stage 1 cannot
move: **compare the set of classfile names we emit against scalac's, for the
`spec-*` tests.**

That check is already one of this project's detection methods (see
[`docs/testing.md`](testing.md)), it is cheap, and it fails on exactly the thing
stage 1 does not do. Until `Foo$mcI$sp` appears in our output, the ledger says
so, whatever `pos` reports.

It is `tests/spec_classfiles.sh`. It compiles each of the 37
`test/files/pos/spec-*.scala` twice — once with real scalac 2.13.16, once with
us against the same jar — and diffs the *sets of classfile names*. It takes
about 40 seconds.

```
tests=37 match=2 differ=26 no_compile=9 skip=0
classfiles scalac emits that we do not: 737
specialized classes ($sp): scalac=700 scala-rs=0
LEDGER: RED -- specialization is not implemented (stage 1 only)
```

Read the two lines together. `pos` says 28 of these 37 compile; the ledger says
2 of those 28 produce what scalac produces, and that **scalac emits 700
specialized classes across this suite where we emit none**. The two tests that
match are the ones whose `@specialized` selects nothing to specialize.

What the ledger proves is narrow, and worth saying plainly: it compares
*names*. It does not load, verify or run either side's classfiles, and it says
nothing about whether the bodies inside a specialized class would be right. It
is the cheapest check that fails on exactly the thing stage 1 does not do, and
it will go green only when stage 2 exists.

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

The `spec-*` tests are also what stage 2 will be scored on: the ledger
(`tests/spec_classfiles.sh`) goes from 2 matching to 37, and the 700 `$sp`
classes scalac emits over that suite have to appear.

## Order

Stage 1 was small, self-contained, touched the parser and the symbol table, and
unblocked 78 tests that mostly have nothing to do with specialization —
including 17 `run` tests whose real subject is something else we might be
getting wrong. Getting them running was worth more than the specialization work
itself in the short term, because a `run` test that cannot compile is a defect
we cannot see: nine of those seventeen now compile, run, and print the *wrong*
answer, which is nine defects that were invisible a day ago.

Stage 2 is a phase, and should be planned as one. Its `neg` tests are already
identified: `spec-overrides`, `t4417`, `t4541`, `t5564`, `t9014` all expect an
error that only the phase produces, and all five are now accepted.
