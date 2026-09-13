# Running the cats and gitbucket code we emit (`agent/runharness`, `agent/rhfix`)

`tests/cats_run.sh` + `tests/catsrun/` and `tests/gitbucket_run.sh` +
`tests/gbrun/` are differential **execution** harnesses, built on the model of
`tests/slick_run.sh`: each compiles its project twice (scala-rs and real scalac
2.13.16), compiles a set of ordinary client programs with real scalac, and runs
them against both builds comparing stdout byte for byte. Each client is compiled
*both* ways, which separates a codegen defect (our classes run differently) from
a pickle defect (real scalac cannot use our `ScalaSignature` at all).

Before `agent/runharness` nothing had executed one instruction of either
project. Eleven defects came out of the first runs; five more were left as an
expected-failure ledger, and `agent/rhfix` cleared all five. Both harnesses now
report

```text
tests/cats_run.sh       progs=8 ok=8 diff=0 fail=0 classes=2977 pickle_fail=0 known_fail=0 new=0
tests/gitbucket_run.sh  progs=6 ok=6 diff=0 fail=0 classes=1317 pickle_fail=0 known_fail=0 new=0
```

and both ledgers (`KNOWN_WHY`) are empty. The ledger is still checked in both
directions, so a program that starts failing fails the gate.

Sections 1-5 below are the five entries, each with the reduced program that
showed the defect and the mechanism that fixed it. Section 6 is still open.
Regression tests: `crates/cli/tests/rhf.rs` with `tests/fixtures/rhf_*.scala`.

---

## 1. A redundant mixin forwarder overrode a `final` trait accessor — fixed

Harness: `tests/cats_run.sh Chains`. Fixture: `rhf_finalmixin`.

cats' `CoreDurationInstances` declares `implicit final val
catsStdShowForDurationUnambiguous`. `AllInstancesBinCompat` is an abstract
*class* that mixes it in, so it carries the field and a `final` accessor --
in scalac's output and in ours, identically. `cats.instances.package$all$` then
extends that class, and we emitted the accessor a second time there, so loading
the class failed before any code ran:

```text
java.lang.IncompatibleClassChangeError: class cats.instances.package$all$
  overrides final method
  cats.instances.AllInstancesBinCompat.catsStdShowForDurationUnambiguous()Lcats/Show;
```

Reduced to three declarations:

```scala
trait T { implicit final val v: String = "v" }
class Base extends T          // carries the field and the final accessor
object Leaf extends Base      // must NOT declare `v()` again
```

`javap -p 'Leaf$'` is the whole test: scalac emits `MODULE$` and `<init>` and
nothing else; we emitted `v`, `v()` and `T$_setter_$v_$eq`, and the program died
in the class loader.

**Mechanism.** nsc's mixin phase only ever visits `Symbol.mixinClasses`,
`ancestors takeWhile (superClass != _)` -- the prefix of the linearization
*before* the superclass. Everything from the superclass onward is that class's
business: it carries the field, the accessor, the `$init$` call and the
forwarder, and a subclass that repeats any of them overrides what it emitted.
We walked the whole linearization in seven places. `Gen::mixin_traits`
(`crates/backend/src/gen_trait.rs`) is that prefix, and `mixin_val_fields`,
`mixin_lazy_vals`, `binary_mixin_lazy_vals`, `mixin_member_modules`,
`emit_trait_val_accessors`, `emit_mixin_forwarders` and
`mixin_init_calls` (`gen_object.rs`) all use it now. The old
`superclass_implements` test -- which skipped a past-the-superclass forwarder
only when the superclass *declared* a matching method -- is gone: a class that
merely inherits the member from the trait has the forwarder too, and the
symbol table does not record that.

## 2. A higher-kinded abstract type member lost its arguments in the pickle — fixed

Harness: `tests/cats_run.sh Monoids`, and `Chains` on the pickle axis.
Fixture: `rhf_pickle_lib` / `rhf_pickle_client`.

`cats.data.NonEmptyChain[A]` is `NonEmptyChainImpl.Type[A]`, where `Type` is
`type Type[+A] <: Base with Tag` -- an abstract type member with a parameter and
only an upper bound, the no-boxing newtype pattern. Real scalac reading our cats
said

```text
error: value head is not a member of Type
error: type mismatch;
 found   : cats.data.NonEmptyChain.Type
 required: cats.data.NonEmptyChain.Type[A]
```

Three separate losses, each reduced on its own:

```scala
object A { type Abs[A] <: AnyRef; def mkAbs[A](x: A): Abs[A] = ??? }
// scalac: found q.A.Abs[Int];  ours: found q.A.Abs
```

```scala
private[nt] trait NT { type Type[A] <: Base with Tag }
object NImpl extends NT { implicit def ops[A](v: Type[A]): NOps[A] = … }
// ours: `found: Type[Int] (in nt.NImpl); required: Type[Int] (in <none>)`
```

```scala
package object nt { type N[A] = NImpl.Type[A] }
// ours: `NT.this.Type[Int]`, whose implicit scope has no `NImpl.ops`
```

**Mechanism**, all in `crates/backend/src/pickle.rs`:

* `Applied { ctor: TypeMember, args }` wrote only the bare `TypeRef`: the `_ =>
  pickle_type(ctor)` fallback dropped every argument for anything but a `Class`
  or a `TypeParam` head. `pickle_type_member_ref` now carries them.
* A type member whose declaring class is *not* in this pickle used to be minted
  as a fresh root-owned `TYPEsym`, one copy per class file, so two pickles
  disagreed about what one type was. `external_type_member_ref` writes a real
  external reference instead -- owned by the class that *declares* the member,
  because nsc's unpickler resolves an `EXTref` with `owner.info.decl(name)` and
  not `member(name)` -- under the `this` of the class this pickle is written for
  when that class inherits it (`root_class`, `inherited_this_prefix`).
* `object NonEmptySetImpl extends Newtype` reaches `Type` through the object, and
  that object is what puts `catsNonEmptySetOps` in the type's implicit scope.
  The typer dropped the prefix: `Typer::module_path_type_member`
  (`crates/typer/src/check_types.rs`) now records a `path_member` over the module
  for an *inherited, deferred* type member named through it, and
  `Pickler::module_path_head` writes that module as the prefix. `path_member`
  itself no longer enters the symbol it mints in its path's scope -- under a
  module that was a second `type Type` the object appeared to declare, which the
  kind/bounds override check and the pickle both read as one.

## 3. A lambda over a pattern-bound `for` lost its captured outer — fixed

Harness: `tests/cats_run.sh NaturalTransforms`. Fixture: `rhf_patternouter`.

`cats/arrow/FunctionKMacros.scala` contains

```scala
for (typeArg @ TypeTree() <- typeArgs) if (typeArg.original != null) c.abort(...)
```

inside `class Lifter[C <: blackbox.Context](val c: C)`. `TypeTree` is
`c.universe`'s extractor, so the `withFilter` predicate the pattern desugars to
reads `c` off the enclosing `Lifter` -- but only *inside the pattern*. We emitted
the body method with one parameter, `aload_0` read the element, and real scalac
running our macro class file died:

```text
java.lang.ClassCastException: class scala.reflect.internal.Trees$TypeTree
  cannot be cast to class cats.arrow.FunctionKMacros$Lifter
    at cats.arrow.FunctionKMacros$Lifter.$anonfun$0(FunctionKMacros.scala)
```

Reduced, with the same exception and the same one-parameter `$anonfun$0`:

```scala
class Lifter[C <: Ctx](val c: C) {
  def go(ts: List[c.T]): List[String] = {
    val out = scala.collection.mutable.ListBuffer[String]()
    for (t @ c.TT() <- ts) if (c.show(t) != "") out += c.show(t)
    out.toList
  }
}
```

**Mechanism.** `collect_free` (`crates/backend/src/gen_lambda.rs`) had no arm for
the three pattern-only nodes, so `Bind`, `Star` and `Alternative` stopped the walk
and everything under them looked free of `this`. They recurse now.

## 4. `OptionT` instance ambiguity — fixed with 2

Harness: `tests/cats_run.sh Transformers`.

```text
error: ambiguous implicit values:
 both method catsDataMonadErrorMonadForOptionT in class OptionTInstances1
 and method catsDataMonadErrorForOptionT in class OptionTInstances0
 match expected type Monad[[β]cats.data.OptionT[[A]Either[String,A],β]]
```

This went away with the refinement half of section 5: the `type` declarations of
a refinement were being dropped from our pickle, which is what the specificity
comparison between the two instance traits turned on. No separate change.

## 5. Refinement `type` declarations were dropped from the pickle — fixed

Harness: `tests/cats_run.sh NaturalTransforms` (the `Representable` half) and
`Transformers`. Fixture: `rhf_pickle_lib` / `rhf_pickle_client`.

`Representable.apply[F](implicit ev: Representable[F]):
Representable.Aux[F, ev.Representation]` -- the `Aux` pattern, where
`Aux[F, R] = Representable[F] { type Representation = R }` -- pickled as a plain
`Representable[F]`, so

```scala
val rep = Representable[Function1[Boolean, *]]
rep.index(rep.tabulate[Int](b => if (b) 1 else 0))(true)
```

got `found: rep.Representation, required: Boolean` from real scalac. Reduced:

```scala
trait R[F] { type Rep; def tab(f: Rep => Int): F }
object R {
  type Aux[F, X] = R[F] { type Rep = X }
  def a1[F](implicit ev: R[F]): R[F] { type Rep = ev.Rep } = ev
  def a4[F]: R[F] { type Rep = Boolean } = null
}
// scalac: `R[String]{type Rep = ev.Rep}` / `R[String]{type Rep = Boolean}`
// ours:   `R[String]` / `R[String]`
```

**Mechanism.** `pickle_refined`'s `RefineDecl::Type { .. } => {}` wrote nothing.
`Pickler::pickle_refined_type` writes an `ALIASsym` for `type T = X` and a
`TYPEsym` + `TYPEBOUNDStpe` + `DEFERRED` for `type T <: X`, with a `POLYtpe` over
anonymous parameters for a declaration of kind `> 0`. The second half is the
dependent right-hand side: `ev.Rep` is a path member over a parameter of the very
method being pickled, so nsc's own `SingleType(NoPrefix, ev)` is writable
(`local_term_path_prefix`) -- which needed the parameter entries recorded in
`sym_index` as `pickle_method` writes them.

## 6. `returning … insert` inferred the inserted id as `Nothing` — fixed

Harness: `tests/gitbucket_run.sh Utils`. Fixture: `rhf_viewtargs`.

gitbucket's `AccountService.createAccount` ends with

```scala
val accountId = Accounts returning Accounts.map(_.accountId) insert account
account.copy(accountId = accountId)
```

`R` in blocking-slick's `implicit class ReturningInsertActionComposer2[T, R](a:
ReturningInsertActionComposer[T, R])` is determined only by the argument. We
inferred it as `Nothing`, so the result was discarded and the `copy` became an
unconditional throw:

```text
ours:    …ReturningInsertActionComposer2.insert:(…)Ljava/lang/Object;
         pop; aload 11; astore 12; new Account; dup; athrow
         -> VerifyError: uninitialized 158 is not assignable to 'java/lang/Throwable'
scalac:  …insert:(…)Ljava/lang/Object;
         invokestatic BoxesRunTime.unboxToLong; lstore_3; lreturn
```

Standalone reduction (`slick` 3.4.1 + `blocking-slick` 0.0.14 on the classpath;
`javap -p -c` on `mk` is the whole test):

```scala
import com.github.takezoe.slick.blocking.BlockingH2Driver
import BlockingH2Driver.blockingApi._

object P9 {
  class Rs(tag: Tag) extends Table[(Long, String)](tag, "R") {
    def id = column[Long]("ID", O.AutoInc)
    def n = column[String]("N")
    def * = (id, n)
  }
  val Rs = TableQuery[Rs]
  def mk(implicit s: Session): Long = {
    val row = (0L, "x")
    val id = Rs returning Rs.map(_.id) insert row
    id
  }
}
```

**Mechanism.** `ReturningInsertActionComposer[T, R]` is an inner class of slick's
profile cake, so both the conversion's parameter and the receiver are a class
type under the as-seen-from *view* that records a prefix (`prefix.rs`).
`unify_conv_tparam` (`crates/typer/src/implicits.rs`) had no arm for that
wrapper and both sides fell through to `_ => None`, leaving every parameter
open. It strips the view on both sides now; whether the conversion *applies* is
still `conv_param_matches`' decision, so nothing new is accepted.

The same program then failed on the pickle axis for an unrelated reason that had
been hidden behind it: `Byte` and `Short` were missing from `type_ref_named`'s
list of `scala`-package names, so every one of them was written as a root-owned
`EXTref`:

```text
error: Symbol 'type <root>.Byte' is missing from the classpath.
  This symbol is required by 'value gitbucket.core.util.StringUtil.value'.
error: multiple constructors for String … cannot be invoked with (Array[<root>.Byte])
```

Reduced to `def enc(value: Array[Byte]): String` in a library compiled by us and
a client compiled by scalac against it. Both lists in `pickle.rs`
(`type_ref_named` and `class_type_ref`) name them now.

---

## Still open

### `Database.forURL` is not found in a client that imports `blockingApi._`

Owner: unclaimed; member lookup through a profile's `api` cake. Noticed while
reducing 6, not covered by a harness program.

```scala
import com.github.takezoe.slick.blocking.BlockingH2Driver
import BlockingH2Driver.blockingApi._
val db = Database.forURL("jdbc:h2:mem:x", driver = "org.h2.Driver")
```

scalac accepts this; we report

```text
error: value forURL is not a member of BasicBackend.DatabaseFactory
```

`Database` resolves to `BasicBackend`'s factory rather than `JdbcBackend`'s. The
harness programs reach `Database.forURL` only through gitbucket's *own* compiled
code, so this does not block them -- but it would block any client of ours.

### A polymorphic alias is dealiased before it is pickled

`type Al[A] = List[A]` in a signature comes back as `List[Int]` where scalac
writes `q.B.Al[Int]`. Cosmetic for acceptance -- the two are the same type -- but
it makes our signatures read differently from nsc's, and a reader that compares
them by name (a macro, a documentation tool) would see a difference.
