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
