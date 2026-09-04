# Reading symbols out of jars and pickles

Four slices from the scala-rs development log, all about where a symbol's type
actually comes from when the class lives in a jar rather than in our own source.
The recurring theme is that a JVM classfile cannot express everything Scala's
pickle can — by-name parameters, higher-kinded applications, companion-vs-class
identity — so anything read from the generic signature instead of the
`ScalaSignature` quietly loses information, and the typer then fails a long way
from the real cause.

### A companion and its class are separate symbols (`agent/companionkind`)

This slice **fixed at the root** the problem `agent/catsyntax` had traced but backed out
of ("the result type of a jar member comes back as a bare `F`").
The tests are in `crates/cli/tests/companionkind.rs` and the fixture prefix is `ckind`.

The measurement went from `files=184 errors=518 files_with_errors=80` to
**`errors=443 files_with_errors=75`** (-75 errors / -5 files).
The three kinds of error this slice went after are **gone entirely**:
`no matching overload for (Function0[A])F` went **8 → 0**,
`value flatMap is not a member of F` went **4 → 0**, and
`value >> is not a member of F` went **4 → 0**.

**1. `find_or_stub_java_class` was building a `SymKind::Class` out of `X$`.**

`find_or_stub_java_class` is the entry point for every JVM name named by a parent list,
a descriptor, or an `InnerClasses` attribute. Handed `cats/effect/kernel/Ref$`,
`java_simple_name` dropped the trailing `$`, created a **`SymKind::Class`** named `Ref`,
and put **the companion's** name (`…/Ref$`) in its `jvm_name`.
One symbol was doing the job of two, so both jobs broke.

- **The trait `Ref` cannot own its own symbol.** `ensure_class("cats.effect.kernel.Ref")`
  returns `None` — "a symbol by that name exists, but its `jvm_name` differs from the key" —
  so the type of `Ref#update` comes from the **classfile's generic signature** rather than
  from the pickle. A JVM signature cannot write `F[Unit]`; it can only write `TF;`, so the
  result type comes out as a **bare `F`**. That was the real identity of slick's
  `value >> is not a member of F` / `value flatMap is not a member of F` /
  `no matching overload for (Function0[A])F`.
- **The object's members land on the trait.** `Ref.of` / `Ref.const` get installed as
  members on the trait side.

Names ending in `$` now build the same shape `install_java_module` does — a `ModuleClass`
(named `Ref$`) and its `Module` (named `Ref`). Lookup of an existing symbol likewise only
considers `Module` for a name with `$` and only `Class` for one without.

The first thing that walks this path is `val Ref = cats.effect.kernel.Ref` in cats-effect's
package object. That getter's descriptor is `Lcats/effect/kernel/Ref$;`, so the moment you
write `import cats.effect.{Async, Ref, Resource}` (line 5 of slick's `BasicBackend.scala`)
a symbol carrying the trait's name but the companion's JVM name was installed.

In `agent/catsyntax`'s scratch branch this made `FlatMap[F]` underivable from `Async[F]`
and came out net worse, but **it does not reproduce on today's main**
(it depends on the `InnerClasses` handling, the refinement conversion, and
`give_stub_its_kinds` that the same slice introduced). This change on its own takes
`errors=518 → 494`.

**2. Reading `scala.*` classes the prelude does not carry out of the pickle.**

The same "members reached through a companion get read from the classfile" problem shows up
**without using cats at all**:

```scala
import scala.concurrent.Future
import scala.concurrent.ExecutionContext.Implicits.global
object Main { def main(a: Array[String]): Unit = println(Future(21)) }
```

`Future.apply` takes its body **by name** (`=> T`), but a JVM generic signature has no
by-name, and can only write `Function0[T]`. The result is
`no matching overload for (Function0[T], ExecutionContext)Future[T] with
arguments (21)`. By-name itself is not broken
(`Option.getOrElse` / `scala.util.Try` / `Using.resource` all work) — those are
**classes the prelude writes out by hand**, and `Future` is not.

`adopt_binary_class` rejected **every** JVM name starting with `scala/`. The reason was
sound: rebuilding a class the prelude assembled from its jar form breaks members that used
to work (for the same reason `ensure_class` rejects it). But the line to draw is not
"is it `scala.*`", it is "**did the prelude build it**". We now record
`st.symbols.len()` right after `install_prelude` in `SymbolTable::prelude_end`, and only
read symbols after that point out of the pickle.
What actually gets adopted is `scala.concurrent.Future` / `Promise` /
`scala.collection.mutable.Growable` / `Builder` / `SeqOps` and the like —
**around 50 classes whose names the prelude never mentions**.

**3. A `this.type` in a pickle means "the `this.type` of the class it was installed on".**

As a side effect of point 2, `scala.collection.mutable.Growable` started being adopted, and
`b ++= xs` started returning `Growable[Int]` (`ctacc_builder` broke).
`PickleSupply::conv` was collapsing `SigType::This` to **`self_ty`, i.e. the class the member
is being installed on applied to its own type parameters**. That was correct back when members
were always installed on the receiver's own class, and became **wrong the moment we started
completing a base class as itself**. It now returns `Type::ThisType(class_sym)` and leaves
the rewrite towards the receiver to the existing `subst_as_seen_from`.
`Growable#++=` against a `Builder[Int, List[Int]]` returns `Builder`.

**Adjacent gaps still open** (not fixed in this slice; see `agent/tail1` for the follow-up):

- **A nested class inside a jar companion** cannot be selected when the companion was
  reached through a package object's `val`.
  Using `object Box { final case class Const[A](get: A) }` via `import tiny2.alias.Box`
  (where `val Box = tiny2.Box`), `Box.of` works but
  `Box.Const` gives `value Const is not a member of Box$`.
  On main not even `Box.of` works, so this is **not a regression**.
  slick's `Outcome.Succeeded(_)` / `Resource.ExitCase.Errored(e)` (6 occurrences) are
  this. The JVM name `Outcome$Succeeded` does not distinguish
  "nested inside the class `Outcome`" from "nested inside the object `Outcome`",
  so fixing it needs either the `InnerClasses` `outer_class_info_index`
  (which `parse_inner_classes` currently throws away) or the pickle.
  → In "`value X is not a member of Y$` (`agent/tail1`)" this turned out to have
  **a different root** than `outer_class_info_index` (`qual.sym` pointed at the val itself,
  and candidates were being assembled from an empty `jvm_name`), and it is now fixed.

### Two copies of the same pickled declaration (`agent/ambigmap`)

Cleanup after a regression introduced by `agent/companionkind`. The tests are in
`crates/cli/tests/ambigmap.rs` and the fixture prefix is `am`.

The measurement went from `files=184 errors=411 files_with_errors=72` to
**`errors=387 files_with_errors=70`** (-24 errors / -2 files).
`ambiguous overload` went **32 → 7**, and of those, `ambiguous overload for map` went
**25 → 0**.

**The symptom.** A perfectly ordinary `map` such as
`pkSyms.map { fs => quoteIdentifier(fs.name) }` was coming out as
`ambiguous overload for map`.

**The cause is "there are two copies of the same declaration", and it has nothing to do with `map` specifically.**

The prelude does not write out `map`. It is declared by
`scala.collection.IterableOps`, and `Seq`, `IndexedSeq` and `Set` all inherit it
from there. `PickleSupply::complete_named` **installs the member on the class that
asked for it** (because that is where the typer will look for it next).
In other words, **which class a copy of `IterableOps.map` lands on depends on which
receiver asked first** — that is, on the program being compiled.

If a `scala.Seq` receiver asks first, one copy lands on
`scala.collection.immutable.Seq`; when a `scala.collection.IndexedSeq` receiver asks
later, nothing is found (since `immutable.Seq` is not one of its parents) and a second
copy lands there too. `scala.IndexedSeq` (i.e. `immutable.IndexedSeq`) **has both of
those as parents, and neither of the two is an ancestor of the other**. As a result

- `drop_overridden` cannot fit them to the "the subclass overrides the parent's member"
  shape, and
- the two differ only in rewritten vocabulary (`Seq[B]` versus `IndexedSeq[B]`), so
  specificity cannot break the tie either,

so every `xs.map(f)` came out as `ambiguous overload`. `map` was simply the most visible
one; `flatMap` / `filter` / `partition` / `foldLeft` had the same shape
(the fixture `am_pickledup.scala` hits all five).

Before `agent/companionkind`, `scala.collection.Iterable` happened to be the one asked
first, so only one copy was ever created. The trigger was that roughly 50 more
pickle-derived classes appeared and **changed the order in which classes get asked**.
The bug itself had been sitting there all along.

**The fix.** As far as nsc is concerned there is one `IterableOps.map`. So
`Symbol::pickled_origin` now records which pickled declaration a copy points at —
**the declaring class, the method name, and the erased parameter descriptors**
(**not the class it was installed on**, since that is exactly what differs between
duplicates). `Check::drop_overridden` runs `collapse_pickled_copies` at the head of the
candidate set and keeps only the first copy for any given `pickled_origin`. Because
`lookup_member` walks parents from the back, the one that comes first is the copy
closest to the receiver (for `immutable.IndexedSeq`, the `collection.IndexedSeq` one —
the one whose result type is `IndexedSeq[B]`).

Symbols with an empty `pickled_origin` (prelude, source, or classfile derived) are left
completely alone. Because we group **by declaration rather than by name**, genuine
overloads stay as two, and if they cannot be resolved we still emit `ambiguous overload`
as before (`am_pickledup_bad.scala`).

### Reading `StringOps` from the jar (`agent/stringops8`)

`"abcdef".zipWithIndex` / `.sliding(2)` / `.groupBy(identity)` / `.sortBy(…)` /
`.collect { … }` and many others were all coming out as `is not a member of String`.
The cause was that `StringOps` was **hand-written in the prelude**: every missing method
had to be added by hand, which structurally guaranteed a never-ending stream of gaps.

**Conclusion: we can move it to reading from the jar. And that was the right fix.**

The reading machinery already existed. `crates/pickle` (the `ScalaSignature` reader) and
`crates/typer/src/pickle_supply.rs` ("supply a member from the pickle, on demand, only if
the prelude does not have it") were both in place, and that is how gaps in `List` and
friends get filled. All that was missing was **the connection**:

- `Check::supply_from_pickle` only ever asked the **receiver's** type.
  The receiver of `"abc".groupBy(f)` is `java.lang.String`, which has no
  `ScalaSignature`, so it always came back empty-handed
  (`[pickle] #groupBy: asking String (java/lang/String)`).
- `Check::search_extension`, which searches implicit-conversion candidates, only did a
  `lookup_member` against the conversion's **result** (`StringOps`) and never asked the
  pickle at all.

So I added one place in `search_extension`: ask the conversion result's pickle, but only
when the prelude has nothing. `pickle_supply`'s three principles (the prelude always wins,
never supply what cannot be represented, never read ahead) are untouched.

This works because the prelude keeps hold of `StringOps`'s **class shell**
(`parents = [AnyVal]` and `ctor_fields = [repr: String]`). `SymbolTable::is_value_class`
is decided by exactly those two, and the backend's `invoke_value_extension` →
`value_extension_desc` implements the 2.13 convention verbatim — build the descriptor from
the symbol's type, prepend the receiver's `Ljava/lang/String;`, and invokestatic
`<name>$extension` — so members installed by the pickle **link correctly as they are**
(a pickle-derived symbol carries the erased descriptor read from the classfile in its
`jvm_name`, and `method_desc_from_sym` prefers that). It also does not collide with the
constraint that `ensure_class` must not rebuild an existing symbol.

One more thing: `Predef.wrapString` did not have `low_priority` set. `javap` confirms that
`wrapString` is declared in `scala.LowPriorityImplicits` while `augmentString` is declared
in `Predef$`, so when both have the member, nsc's rule is that `StringOps` wins. The
comment in `search_extension` described that rule, but with the flag unset it could not
act on it (only the `intWrapper` family had it set). Until this was fixed, `groupBy` failed
as "ambiguous, supplied by both `StringOps` and `WrappedString`".

**Only what the pickle cannot express** stays hand-written, in
`crates/typer/src/prelude_stringops8.rs`. 2.13's `StringOps` has
**overloads that differ only in their return type**, and since `erased_desc` looks members
up by the erasure of the *arguments*, it finds two, cannot tell them apart, and declines to
supply (which is the correct call for `pickle_supply`).
But a declined member then falls through to the lower-priority `wrapString` and comes back
as a `WrappedString`, so `"abcdef".collect { case c if c > 'c' => c }` produced
`Vector(d, e, f)` instead of scalac's `"def"`. **A wrong type is worse than no type**, so
these are declared as **two symbols**, the same way `map` is in `prelude_strmap.rs`:

| What stayed hand-written | Why |
|---|---|
| `collect` × 2 | Overloads differing only in return type (`String` / `IndexedSeq[B]`) |
| `withFilter` and `StringOps$WithFilter` | The result is an ordinary class, and its `map` has the same double erasure |
| `addString` × 3 | The pickled shape of `mutable.StringBuilder` does not line up |
| `apply(Int): Char` | There is no corresponding instance method on the classfile side |

To resolve the `collect` overloads I extended the `Infer.pretypeArgs`-equivalent
pre-typing that `map` uses to cover `PartialFunction` as well (`agreed_pf_param`).
A PF's parameter is a **class**, not a `Type::Function`, so `agreed_lambda_params` bailed
out and the more specific `Char` version won no matter what the case block's body returned.

I also fixed the indexing syntax `"abcdef"(1)`. `s.apply(1)` worked while `s(1)` gave
`value apply is not a member of String`, because the `Apply` path only looked for `apply`
when the receiver was a `Type::Class` and never tried implicit conversions
(`retry_apply_extension`).

Until `StringOps$WithFilter` was added to `is_with_filter_ty`, `withFilter`'s result type
was being overwritten with the **receiver** (`StringOps`, which erases to `String`), so the
following `.map` emitted a `checkcast java/lang/String` against a real
`StringOps$WithFilter` and threw `ClassCastException`.

dual-run: `so8` (the expected output is **real scalac 2.13.16's output verbatim**, matching
under `java -Xverify:all`). The rejecting case is `so8_bad` (merely "resolving" a
return-type-only overload is not enough: a case block returning `Int` selects
`IndexedSeq[B]`, which cannot be bound to a `String`).
**The private runtime (`--no-scala-library`) has no `StringOps` at all**, so `so8.scala`
produces 40 diagnostics there (it is not silently accepted).
slick: `errors=518 → 516`.

### Three small clusters of slick's remaining errors (`agent/tail1`)

This is the result of looking at three independent items in parallel. The test is
`crates/cli/tests/tail1.rs`, and the fixture prefix is `t1`.

The measurement went from `files=184 errors=327 files_with_errors=64` to
**`files=184 errors=305 files_with_errors=63`** (-22 errors / -1 file).
The breakdown for each of the three clusters:

| Cluster | before | after |
|---|---|---|
| `value ExitCase is not a member of Resource$` / `Outcome.Succeeded` family | 6 | **1** (a leftover that only shows up across many files, described below) |
| `value getOrElse is not a member of Product` | 4 | 4 (**still unfixed**, described below) |
| `not found: value fromInt` | 3 | **0** |

The -22 difference includes, on top of the direct contribution of those three clusters
(-5 from 6→1 and -3 from 3→0), the cascaded diagnostics such as `no implicit` that were
collateral damage from `fromInt` not being found.

#### 1. `value X is not a member of Y$` (a jar's companion + a package object `val`)

The `outer_class_info_index` of `InnerClasses`, which the README note in
`agent/companionkind` ("The adjacent gap that remains") named as the cause, **was not the cause**
after all. I extended `parse_inner_classes` to check, and for a class like
`Resource$ExitCase$Succeeded$`, looking at **its own entry** in `InnerClasses` shows the
outer always correctly pointing at `Resource$ExitCase$` (the companion doing the lookup);
the indistinguishable case is never actually hit.

**The real cause was the member-lookup fallback in `type_select`
(`crates/typer/src/check.rs`)**. When nothing was found it called
`complete_binary_member(qual.sym, name, span)`. But when the `Box` of `Box.Const`
is **a package object's `val`**
(`val Box = tiny2.Box`; cats.effect's `package object effect` uses exactly this shape
for `Resource` / `Outcome`), `qual.sym` is
**the symbol of the val itself**, whose `jvm_name` is empty. A candidate assembled
from an empty name (something like `$Const`) naturally matches nothing. `Box.of` (a direct
member of `Box$`, filled in when the jar is loaded) worked while only `Box.Const` (the
companion's nested class) failed for this reason, and with a direct import
(`import tiny2.Box`, where `qual.sym` is the module itself) it does not reproduce.

Changing it to try `recv_ty` first (the val's **type**, which `class_sym_of` can resolve
from a `ModuleRef` to the actual module class) turned up four adjacent gaps
one after another:

1. **The candidate loop in `complete_binary_member` was returning on the first JVM name
   it found**. When **both a class and its companion** exist, as with `Const` / `Const$`,
   the class hits first and `return`s, so the companion (the one that has `apply`)
   never gets installed. `Box.Const(5)` was coming out as
   `value apply is not a member of Const`.
   Changed to try all candidates.
2. **`scala/runtime/Nothing$` in a generic signature was not being turned into
   `Type::Nothing`**. The classfile `Signature` for `case object Canceled extends Outcome[Nothing]`
   cannot write `Nothing`, so it writes
   `Lscala/runtime/Nothing$;` (the runtime placeholder class) instead.
   `jtype_to_type` (`classpath.rs`) treated that as an ordinary class, so the
   `Outcome[Nothing] <: Outcome[Int]` check became
   `is_sub_type(Nothing$_stub, Int)` and failed even for the **covariant** `Outcome[+A]`
   (`type mismatch; found: Canceled$ required: Outcome[Int]`).
   `parse_field_ty` (for descriptors) already did this conversion, so
   I added the same mapping on the generic-signature side.
3. **Type parameters of classes read from a jar were not getting their variance**.
   A JVM generic signature cannot write variance (it is a compile-time-only notion).
   Variance exists only in the **pickle**, yet `adopt_tparam_kinds`
   (`pickle_supply.rs`) carried over only the arity and threw the variance away.
   With just the Nothing fix from 2, `Outcome[+A]` would still actually be treated as invariant
   and the same symptom would remain, so it now sets `Flags::COVARIANT` /
   `CONTRAVARIANT` from `TParam::variance`.
4. **A package object's `val` is just a zero-argument method in the classfile,
   indistinguishable from a `def`**. A `p.T` type such as `Resource.ExitCase`
   (SLS 3.2.3, where `p` must be a stable path) turns into
   "stable identifier required" unless `Resource` is stable.
   Stability exists only in the **pickle's `pflags::STABLE`**, yet
   `adopt_binary_class` ignored the pickle's `MemberKind::Val` entirely
   (handling only `Def`). I added `Val` to what it processes,
   attached `Flags::ACCESSOR` to declarations that have `pflags::STABLE` set, and made
   `ident_is_stable` / `member_is_stable` read that as the grounds for stability.
   On top of that there was an ordering gap where `import_named` (the handling of
   `import p.{Resource}` itself) pinned the raw classfile-derived symbol into the scope
   before the pickle was applied; I closed it by calling
   `adopt_binary_class` earlier, inside the import handling.
   `type_select_is_term_prefix` also **refused** to read a term merely because a type
   side existed, when a type alias and a val share the same name
   (`type Box[A] = …; val Box = …`), so I fixed it to always read the `p` of
   `p.T` as a term (exactly as SLS specifies). To avoid breaking the existing
   precedence for `new Outer.Inner()` (the case where there is only an object and no
   companion val), which lives in `qualified_type_owners`,
   `SymKind::Module` is not included in this decision.

I also added the `complete_binary_member` fallback to `project_from_prefix` (type resolution
for `p.T`), but I narrowed the same kind of fallback on the `type_select` side to
**`Type::ModuleRef` only**. Calling `complete_binary_member` unconditionally on a
`Type::Class` (for example `Type::String`) makes
its `owner.kind == Class` branch call `ensure_java_loaded`, force-loading
**the entire raw classfile** of `java.lang.String`, and then
JDK 11's `lines(): Stream[String]` hid 2.13's deprecated
`StringOps.lines: Iterator[String]`
(`scala_library_dual_run_string_ops4` in `e2e.rs` caught that as a regression,
which is how I noticed the narrowing was needed).

**The adjacent gap that remains**: in slick's real source (`closeStreamIteratorAndRelease`
in `BasicBackend.scala`) exactly **one** `Resource.ExitCase` type annotation still
fails. My own reproduction (`tail1.rs`, with two levels of nesting, going through a
package object, and with a covariant trait) is accepted by real scalac and
by our binary too. I could not shrink it to a single file or to a
combination of a few files; it only reproduces with all 184 files of slick.
I decided further tracking was out of scope for this slice.

#### 2. `value getOrElse is not a member of Product` (**still unfixed**)

The cause is `nextBlobOption() getOrElse(…)` in `slick/jdbc/PositionedResult.scala`
(a block with no return type annotation:
`{ … val rr = if (rs.wasNull) None else Some(r); …; rr }`). **Of the 16 identically shaped
`nextXxxOption()` methods, only 4 — `Blob` / `Bytes` (`Array[Byte]`) / `Clob` / `Object` —**
fail; the remaining 12, including `Boolean` / `Int` / `String` / `Date` / `BigDecimal`,
work.

I built many shrunk versions, going as far as reproducing `abstract class … extends Closeable`, `import PositionedResult.
SqlNullException` (a forward reference to the companion), and the real classfiles of `java.sql.{Blob, Clob,
ResultSet}`, and
every one of them **passed** under both real scalac and our binary
(the lub of `None` / `Some(r)`, the on-demand loading of `Blob` / `Clob`,
the generic overloads of `getObject` — none of the places I suspected reproduce it
on their own). Even adding a few files of slick-internal dependencies such as
SlickException / GetResult / GlobalConfig to get closer, it just got buried under a cascade of
unrelated unresolved errors, and I never reached the reason why only `Blob`/`Bytes`/`Clob`/
`Object` get special treatment.
The way it seems to depend on the state of all 184 slick files is the same shape as the
leftover in 1, but here I do not even have a guess at the true cause.
**I have not put in any speculative stub-like workaround**. As a clue for whoever looks next,
the doc comment in `tail1.rs` records the trial and error of the shrinking.

#### 3. `not found: value fromInt`

This is the shape where, after `import integral._` (the implicit `Integral[T]`), you call bare `zero` /
`one` / `fromInt(n)`. `Numeric[T]` is a standard-library trait whose members exist only in
the **pickle** of the compiled scala-library (the classfile itself has no
corresponding nested class), and
the wildcard-import fallback in `expose_unqualified` (`check.rs`)
only called `complete_binary_member`.
As we saw in 1, that is for "finding a nested classfile", and
**a plain method** like `fromInt` was never findable that way in the first place.
`import_wildcard` (the immediate copy at import time) only picks up "what is already in
`owner.members` at that moment", so `fromInt`, which nobody had touched yet,
did not make it into the copy, and everything rested on the lazy fallback
for when it was referenced later.

The odd part about why this got fixed is that `zero` / `one` did reproduce
(in `crates/cli/tests/tail1.rs::fixtures_t1_wildcard_inherited` I built a minimal reproduction
using **all three** of `zero` / `one` / `fromInt`, and before the fix
**all three** were "not found". In slick's source it looks like
`zero` / `one` happened to be touched earlier in a different form within the same method body,
which is why they worked). The fix was merely to add
`PickleSupply::complete` (the pickle path that ordinary member selection `x.zero` already uses)
to the wildcard fallback in `expose_unqualified` as the second-best option
when `complete_binary_member` fails. Since jvm names starting with `scala/`
are unconditionally allowed
inside `complete_named`, no additional
adopt is needed.

#### What I left alone

I have not touched `agent/mismatch9` (`type mismatch` in general) or `agent/quasi`
(quasiquotes / macros).

#### fixture

`crates/cli/tests/tail1.rs`:

- `a_nested_member_through_a_package_object_val`: uses real scalac to bake
  `t1lib.Box` / `t1lib.Outcome` (the companion's nested `Const`, and
  `case object Canceled extends Outcome[Nothing]` inheriting `Outcome[+A]`)
  into a jar, then compiles and runs user code that touches them only through
  `t1lib.alias` (a package object holding a `type` and a `val` under the same name),
  passing `java -Xverify:all`. It also checks the negative case that rejects
  a missing `bogus` member.
- `real_scalac_accepts_the_same_program`: compiles and runs the same 3 files with real scalac
  alone and confirms the same stdout
  (backing up that the fixture is correct Scala rather than "a quirk of our compiler").
- `fixtures_t1_wildcard_inherited` / `real_scalac_accepts_
  t1_wildcard_inherited`: compiles and runs `tests/fixtures/t1_wildcard_inherited.scala`
  (a loop that uses `zero` / `one` / `fromInt` after `import integral._`)
  with both `--scala-library` and real scalac, and checks that it matches
  `tests/fixtures/expected/t1_wildcard_inherited.txt`.

