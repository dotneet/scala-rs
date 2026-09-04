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
