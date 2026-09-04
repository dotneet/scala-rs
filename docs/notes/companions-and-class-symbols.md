# Companion objects, parent resolution, and class symbols

This file collects three related slices of work on how class symbols and their companions are built and emitted. The first is about silently accepting parent classes and traits that do not exist at all. The second is about a companion's `apply` ending up loaded twice, once from the hand-written prelude and once from the pickle. The third is about companion classfiles never being emitted for local `case class` declarations. Each chapter records the symptom, the root cause, the fix, and how it was verified against real scalac 2.13.16.

### Silently accepting parent classes and traits that do not exist (`agent/parentcheck`)

The bug: a class or object could extend a name that resolves to nothing at all, and we would emit a classfile for it without a single diagnostic. The root cause turned out to be that unresolved names in type position come back as a `Type::Named` placeholder, which is *not* a failure marker (legitimate jar-derived types use it too), and nothing inspected that placeholder before it was stored into `Symbol::parents`.

```scala
object Bogus extends NoSuchThingHere   // both modes emitted a classfile with no diagnostic
class C extends AlsoMissing            // same here
```

Real scalac 2.13.16 says `not found: type NoSuchThingHere`. We said **nothing at all** and wrote a classfile that inherits `java/lang/Object`. As over-acceptance goes, this is one of the heavier cases.

#### Cause

Name resolution in type position (`resolve_type_name`, `crates/typer/src/check.rs`) turns a name it cannot find into a **placeholder** `Type::Named { name }` and returns that. This is **not** a failure marker — the type of a member read out of a jar, where the pickle only wrote a simple name and we have not yet loaded that package, becomes the same `Type::Named` (`crates/typer/src/classpath.rs`). Large parts of the implementation deliberately tolerate this, so we cannot make "seeing a `Type::Named` is immediately an error" the rule.

In an `extends` clause that placeholder went into `Symbol::parents` without anyone inspecting it, and when codegen could not resolve the parent it silently fell back to `java/lang/Object` and wrote the class out. Type arguments (`extends Seq[MissingArg]`) were already being checked by `apply_types`, but the **argument side** passed straight through. Self-types produced `illegal inheritance: self-type G does not conform to MissingSelf` (claiming a nonexistent type is "not conformed to"), `new Missing` produced `not found: value Missing` (the wrong namespace), and `new Missing {}` (an anonymous class) was silent.

#### The fix

Add a single `strict_type_names` flag to `Typer` and set it **only in the places where we know nsc has finished resolving, and where letting it slide means accepting a program scalac rejects**:

- Template parents (the head of `extends`, each item of `with`, the head of `extends P(args)`)
- Self-type annotations
- `new X` / `new X {}` (anonymous classes go through the same path as a parent)

`tree_to_type` recurses, so `extends Seq[Missing]` points at `Missing`, just as scalac does. Diagnostics from the header pass (`parents_pass`) were already being discarded, so legitimate parents that resolve **late** from a pickle or a jar are unaffected (only names that are "genuinely not found" after `expose_unqualified` has tried the enclosing package, `scala._` / `java.lang._`, wildcard imports, and the pickle are in scope here).

For qualified parents (`p.T`), we name the **segment that is actually missing**.

| What you wrote | Diagnostic (matches real scalac 2.13.16) |
|---|---|
| `extends Holder.NoSuch` | `type NoSuch is not a member of object pcq.Holder` |
| `extends pcq.NoSuchInPkg` | `type NoSuchInPkg is not a member of package pcq` |
| `extends java.util.NoSuchJU` | `type NoSuchJU is not a member of package java.util` |
| `extends pkgless.Missing` | `not found: value pkgless` (SLS 3.2.3 — the prefix of `p.T` is a **term**) |
| `extends scala.collection.nosuchpkg.Foo` | `object nosuchpkg is not a member of package collection` |

nsc writes the owner of a missing package segment with a **simple name** (`package collection`) and the owner of a missing type with a **full name** (`package java.util`). We match that too.

`new Obj` (where `Obj` is an `object`) is also `not found: type Obj`. There is no **type** there that can be constructed, so when we were letting it through we emitted a `new` of a module class whose constructors answer nothing.

#### Verification

The fixture prefix is `pc` and the test file is `crates/cli/tests/parentcheck.rs`. For the failure cases we check that they are rejected in **both modes** (private runtime and jar) and that not a single classfile is written. For the success cases we run `java -Xverify:all` in both modes and compare against the output of real scalac 2.13.16.

| Fixture / test | Contents |
|---|---|
| `pc_parents.scala` (success cases) | Parents with arguments, generic parents, `with` mixins, self-types, anonymous classes, qualified parents, parents reached through a type alias. All of them travel the same path as an unresolved name, so if the rule is too broad this is where it breaks |
| `pc_extends_bad.scala` | The head of `extends`, an item of `with`, the head of an applied parent, and its type arguments (6 cases, the same 6 as scalac) |
| `pc_selfnew_bad.scala` | Self-types (each item of `A with B`), `new Missing`, `new Missing {}`, `new Obj` |
| `pc_qualified_bad.scala` | The 6 cases in the table above |
| `pc_new_of_a_missing_type_is_not_a_missing_value` | That `new Missing` does not fall back to `not found: value` |

slick (`tests/slick_measure.sh`) is **unchanged at `errors=257 files_with_errors=63`**, with zero new false diagnostics. Three existing cases simply moved from `not found: value DumpInfo` / `value Mapper` to `not found: type …`, i.e. to the **correct namespace**.

#### Remaining

- **The diagnostic wording for `Ordering[String].compare(1, 2)` has drifted away from scalac** (the rejection itself is still intact). With the jar implicit supply from `agent/tail2`, pickle-derived candidates line up next to the prelude's `compare`, and the single-candidate `type mismatch; found: 1 required: T` (word for word what scalac says) turned into `no matching overload`. This looks like a new seam where duplicates with the same erasure slip past the supply gate (`agent/setapply2`). A wording-only problem.

- ~~`new T` (a type parameter) / `new A` (an abstract type member) still pass silently.~~
  Fixed in `agent/eqtail` (described below).
- Qualified names fall back to re-resolving with the **bare name** when `lookup_qualified_type` fails (because that path cannot model the prefix). As a result, `p.Foo` will still bind to an unrelated top-level `Foo` if one exists. A diagnostic is only produced when both attempts fail.
- Type position in general (`val x: Missing`, `def f(x: Missing)`, type arguments in general) is out of scope for this slice. The `Type::Named` placeholder is also a legitimate jar-derived type, so closing that off requires distinguishing "unresolved" from "not yet loaded" at the type level.

### A companion's `apply` was loaded twice, from the prelude and from the pickle (`agent/setapply`)

The bug: after any member-side `apply` call on a `Set`, a subsequent `Set(...)` construction reported `ambiguous overload`. The root cause turned out to be that the pickle supply installed a second copy of `Set$.apply` alongside the hand-written prelude one, and neither the pickled-copy collapsing nor the override rules could merge them because one copy has no `pickled_origin` and both share the same owner.

```scala
val u: Set[String] = Set("x")
val b = u("x")          // completes SetOps.apply(A): Boolean through the member path
println(Set("admin"))   // error: ambiguous overload for apply with arguments ("admin")
```

Without the second line, the third line compiles. Real scalac 2.13.16 accepts both and prints `Set(admin)`.

#### Cause

The `apply(elems: A*): Set[A]` of `object Set extends IterableFactory[Set]` is **hand-written** in `crates/typer/src/prelude.rs` (`add_set`), so that `Set(1, 2, 3)` works even under `--no-scala-library`. That symbol has no `pickled_origin` (that marker is only set when a symbol was supplied from a pickle).

`u("x")` demands `apply(A): Boolean` as a **member** of `Set[String]` (declared on `SetOps`). That is not in the prelude, so `Check::ensure_apply_supplied` calls `PickleSupply::complete` to fill it in from the jar. `complete` "**always** asks the companion too, even when something was found on the class side" — that is by design, so that a class like `scala.math.BigDecimal`, which has only an instance-side `apply`, does not end up hiding the companion's 7 overloads (this comes from `agent/companionkind`). The companion's own module class is among the things this "always ask" reaches, and so `apply` gets completed on `Set$` **for the first time at that point**. But `Set$`'s `apply` is **already there** from the prelude, so this is exactly the "two copies of the same declaration" situation (`agent/ambigmap`) — except that this time the second copy comes not from a different **class** but from a different **origin** (the pickle), landing on the same class.

`collapse_pickled_copies` from `agent/ambigmap` only bundles copies where `pickled_origin` is set on **both**. Hand-written prelude symbols are deliberately out of scope (symbols with an empty `pickled_origin` are "never touched at all" — the boundary line that keeps genuine overloads from being wrongly deleted). The override rules in `drop_overridden` do not fire either when the two have the **same owner** (`Set$` itself), since they only apply to the shape "a subclass overrides its parent". The result is that the prelude version and the pickle version of `Set$.apply` both survive with nobody bundling them, and `Set(...)` reports `ambiguous overload` **exactly as often as the member-side completion ran first** — it reproduces as soon as one instance-side call like `u("x")` comes first, and does not reproduce otherwise. The order-dependent symptom is the same shape as `agent/ambigmap`.

#### The fix (first attempt, caused a regression)

The first version only added a check to `PickleSupply::install` (`crates/typer/src/pickle_supply.rs`), immediately before the member read from the pickle is actually installed: **if the same class already carries a member with the same name and the same erased parameter shape as a hand-written prelude symbol, the pickle version returns `None` and supplies nothing**. Whether a symbol counts as "a hand-written prelude symbol" is decided by `pickled_origin` being empty **plus** its symbol ID being less than `SymbolTable::prelude_end`. (An empty `pickled_origin` is not unique to prelude symbols: the **provisional** symbols that `adopt_binary_class` reads from the classfile reader have one too. Judging on an empty `pickled_origin` alone, without also looking at the ID, meant things like `scala.Equals.canEqual` could no longer be replaced by the pickle's precise type, and I once produced a regression where **every case class** became `needs to be abstract`. The `< prelude_end` condition prevents that.)

This broke **two other things** in the post-merge full verification (details in the next section). Both had the same shape: returning `None` and thereby **supplying nothing** made the prelude member **invisible** to callers that only read the **return value** of `complete_named` (such as the companion merge in `PickleSupply::complete`). The prelude version had been sitting in `class_sym.members` the whole time, so paths that look it up directly with `lookup_member` were unharmed, but paths that build their candidate set purely by accumulating `complete_named` return values could only interpret it as "nothing came back".

#### The fix (second attempt, the current version)

Instead of `None`, we now **return the already-installed prelude symbol as-is** (`Some(blocker)`). No new symbol is created and `class_sym.members` is not touched, but from the caller's point of view this is **indistinguishable** from "the pickle itself answered with this prelude declaration" — every path that reads `complete_named`'s return value now behaves as if the prelude member had been there all along.

The comparison is still on shape (erased parameters), not name. Pickle members with the **same name but a different shape** are supplied as new symbols exactly as before (`Set[A]`'s member `apply(A): Boolean` and the companion's `apply(A*): Set[A]` differ in both owner and shape, so this check never touches them).

#### The two cases the first version broke

1. **`agent/oshadow`** (`oshadow_order_independent` / `oshadow_bad_is_rejected`). `scala.math.BigDecimal` has three hand-written prelude overloads: `apply(Int)` / `apply(String)` / `apply(java.math.BigDecimal)` (in `crates/typer/src/prelude_oshadow.rs`, because they are used to turn JDBC results into Scala values). When typing `BigDecimal(2)`, `Check::type_select` takes the merged result of `PickleSupply::complete` — which "asks the pickle about both the class side (the instance's `apply(MathContext)`) and the companion side" — and uses it directly as `found` (the candidate set), then caches that on `fun_sym` via `Check::record_overload_group`. In the first version, all three of `apply(Int)` / `apply(String)` / `apply(java.math.BigDecimal)` went **entirely missing** from that merged result (returning `None` meant they never appeared in `complete_named`'s return value, so `complete`'s merge could not find them either). As a result `BigDecimal(2)` was only ever compared against `Long` / `Double` / `BigInt`, none of which can win decisively, so it became `ambiguous overload` — and worse, that wrong error was decided and recorded **before** `Check::widen_with_companion` (the last line of defence, which only runs on `OverloadPick::None` and re-widens the candidates with the companion's members) ever got a chance to run once, because `Ambiguous`, unlike `None`, is not eligible for `widen_with_companion`. In the second version (`Some(blocker)`), all three are in that merged result from the start, so `BigDecimal(2)` resolves uniquely to `apply(Int)` on the first pass and never even needs to go through `widen_with_companion`.
2. **`agent/uniteq`** (`ue_enum_scala_library`). For `scala.Enumeration`, `Value` (no arguments) is hand-written in the prelude and the remaining three overloads (`Value(Int)` / `Value(String)` / `Value(Int, String)`) are hand-written in `crates/typer/src/prelude_enum.rs`. `values` / `withName` / `apply` / `maxId` are read from the pickle via the `library_ancestors` fallback in `PickleSupply::complete` (which only runs when a user class has a library ancestor). The first version had the same shape here: parts of the `apply` / `Value` family being completed onto `Enumeration`'s own class dropped out of the return value, and member resolution for `object Color extends Enumeration` broke. The second version fixed this at the same time.

#### Verification

The fixture prefix is `sa` and the test file is `crates/cli/tests/setapply.rs`. `sa_setapply.scala` folds into one file: the `Repo` trait's `xs(tag)` (which force-completes `SetOps.apply(String): Boolean` through the member path — the exact shape of the original report) followed by `Set(...)`, then the reverse order, then the same-shaped cases for `Map` / `List` / `Seq`. It runs under `java -Xverify:all` both in the `--scala-library` dual-run and in the execution-result diff against real scalac 2.13.16. The same fixture also pins down that the member `apply` of `Set[String]` still returns `Boolean` as before (`u("x")` / `v(2)` / `m("a")` / `xs(1)` / `ys(0)`). `Repo`'s element type is fixed to `String` rather than `A` (the trait's type parameter), because putting `xs(tag)` through with an abstract type argument trips **a separate, pre-existing bug unrelated to this one** — that neither the fixed-arity nor the varargs parameter list can be settled by specificity — and would emit an extra, unrelated `ambiguous overload for apply` (see "Known remaining issues" below). Since the private runtime has no `scala.collection` pickle (and therefore no room for a double load), `sa_setapply_without_the_library_is_diagnosed` checks that under `--no-scala-library` `Set` **does not pass silently** but is diagnosed as `not found: type Set`. `sa_setapply_bad.scala` pins down that two genuinely existing overloads with no common parent (`Pick.apply` on a `Cx` that implements `Ax` / `Bx`) are not bundled, stay as two, and — when nothing settles it — produce `ambiguous overload` just as scalac does. That is the guarantee that what we fixed is the "shape", not the "name".

For the second version, on top of the above I ran `--test overloadshadow --test uniteq --test ambigmap --test mutcoll --test conform` all in the foreground and confirmed everything is green, including the two cases the first version broke.

slick (`tests/slick_measure.sh`) went from `files=184 errors=257 files_with_errors=63` to **`errors=241 files_with_errors=61`** (−16 errors / −2 files). The original `Set` order dependence itself did not show up in slick's 184 files, but the `agent/oshadow` / `agent/uniteq`-style path — "candidates dropping out of `complete_named`'s return value", which the second version fixed at the same time — was evidently being hit by slick's code as well.

#### Known remaining issues

- The `ambiguous overload` on `java.util.Set.of("x")` (choosing among 10 fixed-arity overloads plus a varargs one) has a **different root**. `java.util.Set` is read directly from a Java classfile (`javaclass.rs`) and never goes through the completion path in `pickle_supply.rs` at all, so it remains out of scope for this fix (already noted under Remaining in the `agent/javanest` README section).
- **Specificity between a fixed-arity and a varargs parameter list is not settled when the element type is an abstract type parameter.** In `trait Repo[A] { def hasTag(xs: Set[A], tag: A): Boolean = xs(tag) }`, `xs(tag)` matches both `SetOps.apply(A): Boolean` (fixed arity) and `IterableFactory.apply(A*): CC[A]` (varargs, inherited by `Set[A]`), producing `ambiguous overload for apply`. If the element type is concrete (e.g. `String`), the fixed-arity side wins correctly. This is a pre-existing bug that is present on plain main under `--scala-library` even before this fix, and is out of scope for `agent/setapply`.

The fixtures for the `agent/eqtail` slice (summoning `Equiv[T]` and the hierarchy edges `Ordering <: PartialOrdering <: Equiv`) use the prefix `eq2` (`eq2_summon` / `eq2_summon_bad`) and, for the same reason, live in `crates/cli/tests/eqtail.rs`. `eq2_summon.scala` folds into one file: `implicitly[Equiv[T]]` (for `Int` / `String` / `Long` / `Boolean` / `BigInt`), a direct reference to `Equiv.Int`, an identity check on the instances via `getClass.getName` (`Equiv$Int$` / `Equiv$DeprecatedDoubleEquiv$`), and the widening assignments that pass `Ordering.Int` into `Equiv[Int]` / `PartialOrdering[Int]`. It runs under `java -Xverify:all` both in the `--scala-library` dual-run and in the execution-result diff against real scalac 2.13.16 (`eq2_summon_matches_real_scalac`). `eq2_summon_bad.scala` pins down that adding the hierarchy edges does not make `implicitly[PartialOrdering[Int]]` summonable (real scalac has no instance for it either), that an `Equiv[Int]` cannot be passed where an `Ordering[Int]` is expected (widening only goes in the `Equiv` direction), and that the companion object itself is not an `Equiv`. Since the private runtime has no classfile for `scala/math/Equiv`, `summon_is_diagnosed_without_the_jar` checks that under `--no-scala-library` `Equiv` **does not pass silently** but is diagnosed as `not found: type Equiv`. The fixtures for the `Ordering#compare` fix in the same slice are `eq2_compare` / `eq2_compare_bad`. `eq2_compare.scala` folds into one file the `compare` / `lt` / `gt` / `lteq` / `gteq` / `equiv` / `max` / `min` of `Ordering[String]` / `Ordering[Int]` plus a generic function taking an `Ordering[T]` (`cmp[T](ord: Ordering[T], x: T, y: T)`), and runs both in the `--scala-library` dual-run and in the execution-result diff against real scalac 2.13.16 (`eq2_compare_matches_real_scalac`). `eq2_compare_bad.scala` pins down that `Ordering[String].compare(1, 2)` / `Ordering[Int].compare("a", "b")` / `Ordering[String].lt(1, 2)` / `Ordering[String].max(1, 2)`, all of which passed silently before the fix, are now rejected for the same reason real scalac rejects them (there is no `--no-scala-library` case, since `Ordering` itself is a hand-written symbol used only for `library_abi`). The fixtures for the `new T` / `new A` fix (the remaining item from `agent/parentcheck`) are `eq2_newtype` / `eq2_newtype_bad`. `eq2_newtype.scala` is the success case confirming nothing broke after the fix (a real class **applied** to a type parameter, `new Box[T](value)`, and `new Self` through a type alias where `type Self = ConcreteNamed`); since it uses no jar functionality it runs under `java -Xverify:all` in **both the private runtime and `--scala-library`**, and is also diffed against real scalac 2.13.16's execution result (`eq2_newtype_matches_real_scalac`). `eq2_newtype_bad.scala` pins down that `new Self` (a bare reference, inside the declaring trait itself, to an abstract type member with no `=`) and `new T` (a method type parameter), both of which passed silently in both modes before the fix, are now rejected in both modes with real scalac's exact wording: `class type required but Named.this.Self found` / `class type required but T found` (`eq2_newtype_bad_is_rejected_private_runtime` / `_scala_library`). slick's sources do not reference `Equiv` / `PartialOrdering`, so the `tests/slick_measure.sh` numbers are unchanged before and after this slice.

### The companion classfile for a local `case class` was never emitted (`agent/localcc`)

The bug: a `case class` declared inside a method body typechecked fine but died at runtime with `NoClassDefFoundError` for its companion. The root cause turned out to be that method-body declarations go through a different emission path (`Backend::emit_anon_classes`) which called `emit_class` but never `emit_case_companion`.

```scala
def main(a: Array[String]): Unit = {
  case class P(n: Int)
  println(P(1))       // typechecks, but at runtime: NoClassDefFoundError: Main$P$1$
}
```

Typechecking passes (`Typer::ensure_companion` links the companion's symbol properly), but running it fails with `NoClassDefFoundError: Main$P$1$`. This is a **silent miscompilation**.

#### Cause

For a top-level `case class` (or one directly inside a class), `Backend::walk_stats` (`crates/backend/src/gen.rs`) calls `emit_case_companion` right after `emit_class`, emitting the companion's module class with its `apply`. But a `case class` declared **inside a method body** goes through a different path (the `Block` arm of `Backend::emit_anon_classes`), and that path only called `emit_class` — it never called `emit_case_companion` at all. `Main$P$1` was emitted but `Main$P$1$` with its `apply` was not, so `P(1)` (which desugars into a call to the companion's `apply`) became a link error. A local `case object` (which has no companion — the `object` itself is the module) and the surrounding machinery that already handles local `trait` / `class` / `object` (`agent/localtrait`) were unrelated and working correctly.

#### The fix

Added to the `Block` arm of `emit_anon_classes` the same test used by the top-level `walk_stats`: if the `case` flag is set and there is no user-written companion `object` of the same name in the same block, call `emit_case_companion`.

#### One more gap found while verifying (part of the same fix)

After landing the fix and getting `lcc1` (the reproduction of this bug itself) to pass, I checked the "with capture" shape the brief called out (a local `case class` whose body reads a local variable from the enclosing method) and found **one more** silent miscompilation:

```scala
def main(a: Array[String]): Unit = {
  val base = 10
  case class Q(n: Int) { def total: Int = n + base }
  println(Q(5).total)   // typechecks, but at runtime: NoSuchMethodError: 'void Main$Q$1.<init>(int)'
}
```

The `Q` class itself is correctly turned into a constructor with a capture field by the existing general machinery (`crates/typer/src/anon_capture.rs`), giving `<init>(int, int)`. But the companion's `apply` (`emit_case_apply`) looks only at `ctor_fields` when building `new Main$Q$1(n)` and knows nothing about the capture arguments. Real scalac (confirmed with `javap`: `Cap$Q$2$` holds a `private final int base$1` of its own and **constructs a new companion on every call** rather than using a `MODULE$` static singleton) does exactly what a normal local `object` does when it is obtained once through `scala.runtime.LazyRef` — "a local type with captures is a fresh instance every time" (`check_local_objects` in `crates/typer/src/localobj.rs` already rejects this shape on the local `object` side).

Supporting this shape would require rebuilding the companion's `MODULE$` static-singleton representation from scratch (the `LazyRef` equivalent); it is a separate implementation problem, worth a slice of its own, and distinct from the actual subject of this slice (the companion **not being emitted**). Following the policy `localobj.rs` has already established (reject unimplemented shapes with a diagnostic; do not pretend they work), I added `check_local_case_class_captures` (`crates/typer/src/localobj.rs`). It runs **immediately after** `mark_anon_captures` has filled in `Symbol::captures` (`crates/driver/src/lib.rs`), and if a local `case class` has a non-empty `captures` it emits a diagnostic and stops compilation. That turns "typechecks but dies at runtime" into "does not compile".

#### Verification

The fixture prefix is `lcc` and the tests live in a new file, `crates/cli/tests/localcc.rs`. `lcc1.scala` is the brief's reproduction itself (constructing `P(1)` plus a `case P(x) => …` pattern match), `lcc2.scala` is a local `case object` (a regression guard for something that was never broken), and `lcc3.scala` has two methods that each declare a `P` of the same name (checking that separate classes **and** separate companions are emitted and do not leak into each other). All three run under `java -Xverify:all` in both `--no-scala-library` and `--scala-library` modes, with the expected values being real scalac 2.13.16's execution results (`tests/fixtures/expected/lcc{1,2,3}.txt`). I confirmed that running `lcc1` on the pre-fix `main` (with the `emit_case_companion` call removed) fails with `NoClassDefFoundError: Main$P$1$`. The capture shape is `lcc4_bad.scala` (a `compile_fails` test pinning down that compilation fails with the diagnostic `not implemented: a local case class Q that reads a local of the enclosing method …`). The two tests `local_case_class_companion_has_apply` / `same_named_local_case_classes_get_separate_companions` use `javap` to inspect the shape of the classfiles actually emitted (that `Main$P$1$` exists and has `apply(int): Main$P$1`, and that `lcc3` emits `Main$P$1` / `Main$P$2` as two distinct companions).

I ran `--test localcc --test localtrait --test ctorstmt --test quasi --test product --test companionkind --test outer --test nestedobj` in the foreground: all 64 + 6 tests are green (`quasi.rs` also includes the tests for item 2 of this slice).

#### Known remaining issues

- **A case class companion has no actual `unapply` implementation.** `namer_class` in `crates/typer/src/check.rs` (where the companion is synthesized) only creates the symbol for `unapply` without setting its `.ty`, and `crates/backend/src/gen.rs` has `emit_case_apply` but no `emit_case_unapply`. Calling `P.unapply(P(1))` **explicitly** on a top-level `case class P(n: Int)` typechecks and then fails at runtime with `NoSuchMethodError: 'scala.Option P$.unapply(P)'` (the pattern match `p match { case P(x) => … }` itself is unaffected and works, because it goes through a different path that reads the fields directly). This is a pre-existing, separate gap common to top-level case classes, not just local ones, and is out of scope for this slice.
- **The shape where a local `case class` captures a local variable from the enclosing scope is still rejected with a diagnostic** (see "One more gap found while verifying" above). It needs a `LazyRef`-equivalent implementation.


### Fifty-eight classfiles scalac writes and we did not (`agent/missingclasses`)

Comparing the class**file name sets** of scala-rs and scalac 2.13.16 over
slick's 184 sources (nested names normalised to `$` on both sides) gave 1170
common, 382 ours only, 328 theirs only. Of the 328, 63 were not
anonymous-class naming noise, and they had three roots -- none of which shows
up when slick is compiled on its own, because everything resolves inside one
run. They only bite a **separate** compilation, which is what a compiler
producing a library is for.

The first thing to establish was that they bite at all. A three-line library
compiled by scala-rs and consumed by real scalac settles it:

```scala
package myp
package object util { val greeting: String = "hi"; def twice(n: Int): Int = n * 2 }
class Box(val a: Int, val b: Int = 7)
class Ops(val x: Int) extends AnyVal { def inc: Int = x + 1 }
```

```
$ scalac -classpath LIB_RS use.scala
use.scala:3: error: object twice is not a member of package myp.util
use.scala:3: error: object greeting is not a member of package myp.util
use.scala:4: error: not enough arguments for constructor Box: (a: Int, b: Int): myp.Box
```

and, once the package object was fixed, on the value class:

```
error: java.lang.AssertionError: assertion failed:
  no extension method found for:  method inc:Int
        during phase: globalPhase=erasure, enteringPhase=refchecks
```

So: not cosmetic. scala-rs was writing libraries that scalac cannot use.

#### Root 1 -- a package object is two classfiles, and the pickle is on the other one

nsc compiles `package object p` to `p/package$.class` (the module) **and**
`p/package.class` (the mirror). `javap -v` on both says which one matters:

| classfile | attributes |
| --- | --- |
| `slick/ast/package$.class` | `ScalaInlineInfo`, `Scala` (the bare marker) |
| `slick/ast/package.class` | `ScalaSignature` (the pickle), `ScalaSig` |

The pickle is on the **mirror**. `emit_module` in `crates/backend/src/gen.rs`
had an explicit `&& name != "package"` guard on the mirror-class call, added
in passing back in `734be89` with no reason recorded, so scala-rs shipped no
pickle for a package object anywhere. That is the whole of `object twice is
not a member of package myp.util`: scalac found the module classfile, found
no signature, and had nothing to read. Dropping the guard emits the mirror
with the pickle on it (mirror classes already carry one -- that is how
`object Lib` works), and the consumer compiles and runs. **12 classfiles.**

#### Root 2 -- a value class's `$extension` methods live on its companion

nsc's `extmethods` phase rewrites a value class's methods to
`name$extension` statics-in-spirit and **declares them on the class's
companion module**, synthesizing that module when the source wrote none.
`extmethods` runs *before* `pickler`, so those declarations are in the
signature every later compilation reads. `ExtensionMethods.extensionMethod`
looks the method up in `imeth.owner.companionModule.info` and asserts if it
is not there -- the `AssertionError` above.

We put the `$extension` methods in statics on the value class itself, which
is what our own call sites use and is fine inside one run. The fix keeps
that and adds what nsc's ABI needs:

* `crates/typer/src/value_companion.rs` declares
  `name$extension[C's tparams, m's tparams]($this: C, m's params): R` on the
  companion, creating the companion when needed. Two details of
  `ExtensionMethods.normalize` fix the shape and both are load-bearing: it
  finds the receiver by the **name** `$this` (`nme.SELF`), and it drops the
  first `clazz.typeParams.length` type parameters, so the class's come
  first. The pass runs after the whole run is typed and before `pickle_all`,
  so nothing it adds can change how anything resolves.
* `gen.rs` writes the companion classfile, its methods forwarding to the
  statics -- one copy of each body, both ABIs working. A written companion
  and a `case class ... extends AnyVal`'s synthetic one get the same
  forwarders, so the classfile never says less than the pickle does.

Fixing the lookup only moved the failure one phase along:

```
warning: an unexpected type representation reached the compiler backend: <notype>
error: Error while emitting use2.scala
```

with the erasure tree reading
`Ops.inc$extension(Int.box(41).$asInstanceOf[<notype>]())`. A value class
erases to the type of its **single parameter accessor**, and nsc finds that
accessor by the flag pair `PARAMACCESSOR | METHOD`
(`Symbol.derivedValueClassUnbox`). `pickle_val` set `PARAMACCESSOR` only for
`case` classes, so scalac could not erase `Ops` at all. Setting it for a
value class's accessor too (`crates/backend/src/pickle.rs`) finished it.
**32 classfiles.**

#### Root 3 -- operator characters were written into classfile names raw

A type's simple name goes through the same `NameTransformer` encoding a
method name does. slick's `object :@` nested in `object TypeUtil` is
`slick/ast/TypeUtil$$colon$at$` for nsc; we emitted `TypeUtil$:@$.class` --
a name no consumer can reference, and not a portable file name either.
`jvm_for_current` (`crates/typer/src/check.rs`) now runs the simple name
through `scala_rs_pickle::names::encode_method_name`, which already existed
for methods. **2 classfiles.**

#### Numbers

slick, class-file name sets against scalac 2.13.16, 184 files:

| | before | after |
| --- | --- | --- |
| common | 1170 | 1216 |
| scalac only | 328 | 282 |
| scala-rs only | 382 | 380 |
| non-anonymous "scalac only" | 63 | 17 |

`tests/slick_measure.sh`: `files=184 errors=0 files_with_errors=0
classes=1596` (1552 before -- 44 new files, and 2 renamed rather than
added). `tests/slick_subset.sh`: `verified=1596 failed=0`, so every new
classfile loads with the verifier on.

The tests are `tests/fixtures/mcls_lib.scala` plus `mcls_main.scala`,
compiled against the *classfiles* rather than alongside the source, by
scala-rs and by real scalac, in `crates/cli/tests/e2e.rs`.

#### What is left

* **12 companions for classes with default constructor arguments.**
  `class SlickException(msg: String, parent: Throwable = null)` gets a
  synthetic `SlickException$` from nsc holding
  `$lessinit$greater$default$2`, plus a static forwarder on the class.
  We have neither: the typer inlines a constructor default at the *call
  site* (`type_default_rhs_here`, and the comment there says so outright --
  "a primary constructor's defaults have no `name$default$n` getters"), and
  the parameter is not marked `DEFAULTPARAM` in the pickle either, so
  scalac reading our `Box` says `not enough arguments for constructor Box`.
  Closing this means doing what nsc does -- synthesizing the getter as a
  real `DefDef` in the companion's body at namer time, so it is typed,
  pickled and emitted like any other method -- which is a namer change, not
  a codegen one. The five remaining `$typecreator` / `$treecreator` names
  belong to the anonymous-class naming bucket, not here.
* **Anonymous-class naming.** 265 of the 328 differences are ours and
  scalac's disagreeing on the *name* of an anonymous or `$anonfun` class,
  and on top of that we join a nested anonymous class's name with `/`
  instead of `$`, which drops it at the output root rather than beside its
  owner. Left alone deliberately; it is its own slice.
* **A mirror class forwards `def`s but not `val` accessors.** nsc's
  `slick/util/GlobalConfig.class` has `public static boolean
  detectRebuild()` and five more; ours has only the one `def`. Same for a
  package object's mirror (`slick/util/package.class` is missing
  `ignoreFollowOnError()`). `emit_forwarder`'s list is built from the
  template's `DefDef`s alone. It costs nothing through the pickle -- a
  Scala consumer reads the accessor off the module -- so it only shows up
  from Java. Pre-existing and not specific to package objects; left alone.
* **An operator-named *method* is not visible to scalac through our
  pickle**: `class Plain(val v: Int) { def ~(o: Int): String }` compiled by
  scala-rs and used by scalac gives `value ~ is not a member of
  myw.Plain`. Pre-existing, reproduces on a plain class, and not about
  missing classfiles -- the classfile is written and the method is in it
  under the right encoded name -- so it was not touched here, but it blocks
  separate compilation the same way the three roots above did. (A module's
  **nested class-like members** had the same shape of problem and were
  fixed on `main` by `ceb9b38`, `agent/testkit2`, while this slice was in
  progress; `object Box { class Inner }` now reaches scalac.)

### Static forwarders onto the companion class (`agent/mirrorfwd`)

Two of the loose ends above turn out to be one bug, and the visible half of
it stops a program from starting at all.

nsc gives a top-level `object Test` a set of `public static` forwarders.
Where they land depends on whether the source wrote a companion:

* **no companion** — nsc synthesizes a *mirror class* `Test.class` whose
  whole method table is those forwarders. scala-rs did this.
* **a companion `class Test` (or `trait Test`)** — there is no mirror class.
  The forwarders go onto the companion's own classfile. scala-rs did
  **nothing**, so `Test.class` had no `main` and `java Test` reported
  "main method not found in class Test".

`scala/scala`'s `run/t363` is that program and nothing else:

```scala
object Test { def main(args: Array[String]): Unit = println("…") }
class Test  { def kurtz() = "…" }
```

The second half is which members get forwarded. `emit_forwarder`'s list was
built from the template's `DefDef`s alone, which is why
`slick/util/GlobalConfig.class` had one static where nsc has seven: a `val`
is not a `DefDef`, and neither is anything inherited.

#### What real scalac 2.13.16 forwards

Read off `javap -p`, one probe per question, not from nsc's source. The
probes are reproduced as tests in `crates/cli/tests/mirrorfwd.rs`.

Forwarded: every **public** member of the module class, *including
inherited* ones — a mixed-in trait's concrete `def`, its `val`; `val` / `var`
/ `lazy val` getters and a `var`'s `x_$eq` setter; every alternative of an
overload; `f$default$1` and the other default-argument getters; a value
class's `plus$extension` statics; a `case object`'s `productPrefix` /
`productArity` / `toString` / … .

Not forwarded:

* `private` members — and `protected` and `private[p]` ones, which is the
  part that cannot be read off the classfile: `protected def prot` and
  `private[p] def bnd` are both **`public`** in `Test$.class`. It takes the
  Scala symbol to tell them apart.
* Anything whose *name* also names a member of the companion class,
  inherited members included. By name, not by signature: with `class Test {
  def clash(): Int }` next to `object Test { def clash(): Int; def
  clashDiffSig(i: Int): Int }`, `clashDiffSig` survives but *neither*
  `clash` does — and with an overload set, one conflict removes all of it.
  `java.lang.Object`'s names count, which is why a companion class
  suppresses a `toString` forwarder that a mirror class would get (`object
  OverrideToString { override def toString = "x" }` alone *does* get
  `public static String toString()`).
* Members merely inherited from `java.lang.Object`.
* Bridges.
* Everything, when the `object` is not top level: `object Outer { class
  Nested; object Nested }` puts nothing on `Outer$Nested`.

A companion **trait** takes them too — scalac writes `public static int
onObj()` straight into the interface classfile, which classfile major 52
allows.

#### The fix

`crates/backend/src/companion_fwd.rs` decides the set;
`gen::add_static_forwarders` writes it, into a fresh mirror class or into
the companion's builder.

The set is read off the **method table just emitted onto the module
classfile** (`ClassBuilder::methods`), not off the symbol table. A forwarder
is an `invokevirtual` against `MODULE$`, so a name the module classfile does
not really carry links and then throws `NoSuchMethodError` at the first
call; picking from what was emitted cannot produce one. It also gets the
inherited members for free, because scala-rs already writes a mixin
forwarder onto the module for every concrete trait member. The Scala
symbols are consulted only for the two questions the classfile cannot
answer: which names are `protected` / `private[p]`, and which names the
companion class uses.

A classfile's constant pool is written when its builder is finished, so a
companion class cannot be reopened to add forwarders later. The `class` and
the `object` are emitted in source order and either may come first, so
`Gen::finish_companion_class` **parks** a companion class's builder until
its `object` has been emitted, and `Gen::deliver_companion_forwarders`
finishes it then. `flush_parked_companions` writes out anything still
parked at the end of the unit — a missing forwarder is bad, a dropped
classfile is much worse. The only effect is where those classfiles sit in
the output list; nothing reads that order.

Emission is driven by the JVM descriptor (`companion_fwd::desc_slots`)
rather than by `Type`, so the forwarder moves exactly the slots the target
declares. That is also how the `Unit`-parameter and `Nothing`-result
special cases in `jvm_slot_sort` / `emit_return` stop mattering here:
`Lscala/runtime/BoxedUnit;` and `Lscala/runtime/Nothing$;` are references in
a descriptor and nothing else needs saying.

`add_static_forwarders` skips a method the target classfile already has
under the same name and descriptor. A value class carries its
`plus$extension` statics *and* has them declared on its companion, so
without that guard `Meters.class` got a duplicate method and the JVM
rejected the whole file.

#### Measured

`tests/scala_corpus.sh` at `CORPUS_SIZE=full`, before → after:
`run` pass **433 → 442**, `pos` 965 and `neg` 634 unchanged, and a
test-by-test diff shows **zero regressions**. The nine are `t363`, `t2127`,
`t3487`, `t5037`, `t5894`, `t9178a`, `t9422`, `t9946b`, `t9946c`. Four more
tests that used to say "main method not found" now fail further along, for
reasons that have nothing to do with this (`t7448` declares `def main` with
a non-`Unit` result, which is partest's business; `t8756`, `t9365` and
`indylambda-boxing` reach their `main` and then hit a serialization, a cast
and an output difference).

#### Known gaps

* **A member inherited from a superclass is not forwarded.** `object Test
  extends Base` with `Base.fromBase` gets no `fromBase()` static, because
  nothing is emitted onto `Test$` for it — the JVM finds it through the
  superclass. Trait members are fine (they come through a mixin forwarder).
  Closing this means going back to the symbol table for the superclass
  chain, and building descriptors there; nothing in the corpus, slick or
  gitbucket needed it.
* **A case class with no written companion gets no forwarders.** nsc puts
  `apply` / `unapply` / `tupled` / `curried` statics on `CC.class`;
  `emit_case_companion` is not wired into this path. It emits an `apply`
  bridge without `ACC_BRIDGE`, which would be forwarded as `public static
  Object apply(Object, Object)` — something nsc never emits — so the bridge
  wants marking before that path is turned on.
