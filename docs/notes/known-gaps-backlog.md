# Known gaps: the cross-slice backlog

This is the running "Remaining" list from the scala-rs development log — the
gaps that were found but deliberately not fixed in the slice that found them.
Each entry names the slice (`agent/xxx`) that found or confirmed it, the symptom
as it actually appears, and usually the root cause as far as it was traced.
Entries struck through were later fixed; the annotation says by which slice.

It is a backlog rather than a write-up, so it cuts across every part of the
compiler: the typer, implicit search, erasure, codegen, the pickle reader, and
the prelude. Items marked "confirmed in `agent/xxx`" were verified to produce a
diagnostic rather than being silently accepted — that distinction is the point
of most of these notes.

### Remaining

- **When the receiver of `t(i) op= x` is an ordinary method call** (`agent/stmtval`).
  `foo.bar(0) += 1` (where `bar` is a method) is an error in nsc as well,
  reported as `UnexpectedTreeAssignmentConversionError`, but our wording is
  still `value += is not a member of …` plus
  `Expression does not convert to assignment because receiver is not
  assignable.` Both of them do error out, so we do not accept it, but
  the diagnostics do not match.

- **`Int`'s `max` / `min` are missing from the private runtime** (confirmed in `agent/stmtval`).
  `n += i max x` passes in jar mode, but under `--no-scala-library` it becomes
  `value max is not a member of Int` (these are `RichInt` members, so as a
  diagnostic this is the correct shape).
- **`+:` / `:+` patterns** (confirmed in `agent/conspat`). `case P(v) +: _` /
  `case _ :+ P(v)` produce `not found: value +:` / `not found: extractor +:`.
  The extractor objects `scala.collection.+:` / `:+` themselves are present neither in the
  prelude nor on the pickle path, and there is no special-casing like the one for `::`. The
  nested-pattern gap (`case P(v) :: t` in this section) is confirmed **not** to affect these two,
  but that is because they fail in type checking before they can run.

- **`Tuple3` and above, plus the `List.apply` / `Seq` extractors, in the private runtime**
  (confirmed in `agent/conspat`). Under `--no-scala-library` the diagnostics `not found: value Tuple3` /
  `value apply is not a member of List$` / `not found: extractor Seq` are
  emitted (they are not silently accepted). This is why `cp_seq.scala` is restricted to
  library dual-run only.

- **The private runtime's `List` has no Scala-style `toString`** (confirmed in
  `agent/conspat`). In jar mode `List(Q)` renders as `scala.collection.immutable.$colon$colon@…`,
  so the `MatchError` message differs between the two modes only for lists
  (`cp_err.scala` compares only the class name there).
- ~~**When a `Unit` member returns `Object` after erasure, the discarded value is left on the stack**~~
  (confirmed in `agent/anonbridge`) → **fixed in `agent/override`** (`ov_unitpop.scala`).
  A `def` with no parameter list (the `get` of `trait Box[A] { def get: A }`) is called through
  a bare `Select` with no `Apply`, so it did not hit the `Apply` arm of
  `unit_stat_leaves_ref` and no `pop` was emitted. `Select` / `Ident` arms have been added.

  What remains is the same shape for **library members**.

  ```scala
  def f(o: Option[Unit]): Unit = {
    o.get                       // invokevirtual get()Ljava/lang/Object; -- no pop
    try { … } catch { … }       // VerifyError: Inconsistent stackmap frames
  }
  ```

  `unit_stat_leaves_ref` restricts itself via `owner_defined_in_source` to **members defined by
  this compilation unit**. On the library side there are paths such as `Using.resource` /
  `Breaks.catchBreak` / `ArrayOps` where the emit side already discards the value, and
  a second `pop` there would underflow the stack. The real fix is to stop inferring the
  decision from the shape of the tree and instead compare the stack height the `Assembler`
  keeps (`asm.stack`) before and after the statement, discarding what is left; but that
  affects every path through `gen_stat`, so this slice does not touch it.

- ~~**A member of an anonymous class or subclass is accepted even when its result type disagrees with the parent's**~~
  (confirmed in `agent/anonbridge`) → **fixed in `agent/override`**.
  `new It[Int] { def next(): String = "x" }` now gives the same
  `incompatible type in overriding` as real scalac (`ov_result_bad.scala`).

- **Lower-bound inference for `B >: Char`, as in `"abc".appended(1)`** (confirmed in
  `agent/stringops8`). scalac takes the lub with `B := AnyVal` and returns
  `IndexedSeq[AnyVal]`, whereas we infer `B := Int` and emit
  `inferred type arguments [Int] do not conform to method appended's type
  parameter bounds [B >: Char <: Any]`. With a `Char` argument,
  `appended('x'): String` passes, so what is missing is lub inference for
  type parameters with a lower bound itself. `prepended` / `:+` / `+:` / `concat` are the same shape.

- ~~**`LazyZip2` members** (confirmed in `agent/stringops8`)~~.
  `"abc".lazyZip(List(1,2,3)).map(…)` now works thanks to
  the "Higher-order implicit matching for `BuildFrom` (LazyZip)" work
  (`agent/buildfrom2`) (with a `String` receiver, `buildFromString` answers, so the
  result is a `String` too).

- **`StringOps.partitionMap`** (confirmed in `agent/stringops8`).
  `s.partitionMap(c => if (…) Right(c) else Left(c))` becomes
  `(Char) => AnyRef`, and fails because the lub to `Either` cannot be taken.
  Same root cause as the lower-bound inference above.
- **`abstract override` on traits from a jar**. `Symbol::abstract_override` is only set for
  sources we ran through the namer ourselves. Stackable members of a trait read from a pickle /
  classfile are outside the scope of the "is it grounded?" check, so there alone we accept
  without diagnosing (for the same reason, codegen for that super chain is unchanged as before).

- **Type arguments when inheriting a trait's superclass are not filled in on the header path**.
  The implicit superclass completion for `class X extends Loud` (SLS 5.1) is done only on the
  typer's main path. On the header (`sigs_only`) path the trait's parent is still
  `Type::Class { args: [] }`, so completing there gives
  `StatementInvoker takes type parameters` (we actually hit this with slick's
  `class QueryInvokerImpl[R] extends QueryInvoker[R]`).
  Therefore **on paths that look only at the header of another compilation unit, this completion does not apply**.
- **`Product` / `Serializable` on a case class read back through `-cp`** (`agent/product`).
  When a separately compiled `case class Pt(x: Int, y: String)` is used via `-cp`,
  `Pt.tupled` passes (the companion's `AbstractFunction2` is a **superclass**, so it can be
  read from the classfile), but `val q: Product = p` and
  `val s: java.io.Serializable = p` do not. It is the **interface**-side parents that
  get dropped by `-cp` reading, whereas a user-defined trait on the same `-cp`
  (`class Plain extends Marker`) is not dropped. Feeding it a classfile emitted by real scalac
  behaves the same, so this is not a problem with what we **emit**; it is an existing gap on the
  classpath / pickle reading side (around `find_or_stub_java_class` in `classpath.rs` and
  `attach_parents` in `pickle_supply.rs`). Within a single compilation unit,
  both jar mode and private-runtime mode match real scalac.

- **`tupled` / `curried` on a hand-written companion** (`agent/product`).
  nsc does **not** make a hand-written `object P` companion of a case class extend
  `AbstractFunctionN` (confirmed in the classfile), and neither do we.
  scalac still accepts `P.tupled` because it eta-expands the module through `apply`
  and then takes `tupled` from that (deprecated since 2.13.13).
  We do not have that eta expansion, so we emit
  `value tupled is not a member of P$`. A synthesized companion
  (i.e. an ordinary case class with no hand-written `object P`) works through inheritance.
- **The function type `Unit => T`, which takes `Unit` as a parameter, becomes `Function0[T]`**
  (a separate issue found in `agent/unitbox`). `is_unit_tuple` in `crates/parser/src/parse.rs`
  treats an `Ident("Unit")` in type position as an empty parameter list, so
  `def h(f: Unit => Int)` is typed as `() => Int` and `f(())` becomes
  `no matching overload`. In nsc, `Unit => T` is
  `Function1[Unit, T]`, and only `() => T` is `Function0[T]`.
  It is one line in the parser, but it changes how function types are interpreted, so it went into a separate slice.

- **The private runtime has no `scala.runtime.BoxedUnit`** (`agent/patbind`).
  Under `--no-scala-library`, putting `Unit` into `Any` yields `null`, so
  `(x: Any) match { case () => … }` also matches `null`. jar mode matches
  nsc (`pb_nullseq.scala`). The real fix is to add `BoxedUnit` to the private runtime,
  but that means changing the whole box representation of `Unit`.
- **An `object` inside a method (a local `object`) that reads the enclosing scope** (`agent/nestedobj`).
  nsc keeps one instance per call in a `scala.runtime.LazyRef` and passes
  `$outer` and the captured locals to `<init>` (confirmed with `javap -v -p -c`).
  We can still only emit a static singleton, so a local `object` that reads the outer instance
  or the locals of the enclosing method emits
  `not implemented: a local `object` that reads …`
  (`tests/fixtures/nestedobj_bad.scala`). A local `object` that reads nothing from outside
  passes. Fixing it requires codegen for a `LazyRef` local plus capture arguments
  (`agent/lazyref` added a `scala.runtime.Lazy*` cell for local `lazy val`s, along with codegen
  for a lifted accessor that takes that cell, so the groundwork is there.
  What remains is reworking `ModuleDef` into a class that takes `$outer` and the
  captures).
  Note that for an **`object` inside a value class**, scalac itself
  rejects it with `implementation restriction: nested object is not allowed in value class`,
  so we reject it with the same wording (previously we accepted it and got a `VerifyError`).

- **Path-dependent companion `apply` / `copy`** (confirmed in `agent/nestedobj`; same on
  main). For `class Box(val k: Int) { case class Pair(a: Int) }`,
  `bx.Pair(6)` and `p.copy(9)` give `not found: value Pair`.
  `new bx.Pair(6)` passes, and the generated classfile side (an `<init>` taking `$outer`
  first, and `copy` passing its own `$outer`) is implemented, so what remains is only
  companion resolution on the typer side. For the same reason, if an `object` comes first
  in a class body, the companion of a subsequent `case class` also stops being found
  (`case class Holder(k: Int) { object Inner; case class Pair(a: Int) }`).
- **Value discarding for a `Unit` parameter** (SLS 6.26.1, `agent/unitbox`).
  scalac **accepts `f("s")` for `def f(x: Unit)` with a warning**, discarding the value and
  passing `()`. We emit `no matching overload`. It is a matter of touching overload
  resolution, so it is left to that slice.

- **`a(0)` following `def a: Array[T]`** (a separate issue found in `agent/unitbox`, unrelated
  to `Unit`). There is no `apply` insertion on the result of a parameterless method, so it becomes
  `no matching overload for Array[String] with arguments (0)`.
  Binding it to a `val` first and then writing `a(0)` works. It does not depend on the element type.

- **A `var` read from `-cp` looks like a `val`** (a separate issue found in `agent/unitbox`,
  unrelated to `Unit`). Writing `c.w = 5` for a `class C { var w: Int }` in another
  compilation unit gives `reassignment to val w`. It does not depend on the
  field's type (see the comment in `ub_sepuse.scala`).

- **A guard following a value definition in a for-comprehension** (only a diagnostic was added in `agent/mismatch6`; unimplemented).
  `for { m <- ms; q = f(m); if q > 0 } yield q` compiles in nsc. nsc pairs
  the value with the generator's element into a tuple, **filters that stream**, and later
  enumerators receive that tuple by pattern. Our desugaring turns the value into a
  `val` in the lambda body, so there is nothing to filter, and we emit
  `unimplemented: a guard after a value definition in a for-comprehension`
  (`tests/fixtures/mism6_forval_bad.scala`). Fixing it requires the tupling
  desugaring itself.

- **Naming `scala.collection.Seq` explicitly stops `patch` / `filterNot` and friends from being
  narrowed to the receiver's collection** (confirmed in `agent/mismatch6`; same on main).
  **Merely** writing `val c: scala.collection.Seq[Int] = …` causes the real
  `scala/collection/Seq` to be read from the jar, and the raw `Seq` that is `patch`'s declared
  result loses its type arguments. The shortcut for "keep the receiver's collection" bails out on
  `dargs.is_empty()`, so `Vector("a").patch(0, Seq("b"), 0)` becomes
  `found: Seq required: Vector[String]`. On its own it passes
  (`patch_keeps_the_receivers_own_collection` in
  `crates/cli/tests/mismatch6.rs`).

- **5 cases remain in slick where the implicit clause of `toMap` is not filled in**
  (scope narrowed in `agent/mismatch7`; not fixed. Same on main).
  Argument positions (`one(kvs.toMap)`, `(1, kvs.toMap)`), the shape where the expected type is
  directly `Map[K, V]`, and `lazy val m: Map[String, C] = cs.map(c => c.name -> c).toMap` were
  made to work in `agent/mismatch7`. **A minimal reproduction can no longer be produced** ——
  the remaining 5 all occur only inside slick, and the diagnostic splits into two shapes:
  `(<:<[…])Map[K$, V$]` (still a method type) and
  `(<:<[…]) => Map[K$, V$]` (**with an arrow**, i.e. eta-expanded). The latter suggests that
  `adapt` viewed the expected type `Map[…]` as a function (`Typer::function_view` reading
  `Map` as `K => V`), but a form that isolates just that passes, so it is presumably a
  combination with a cascade on the receiver side.
  The one case in `JdbcModelBuilder.scala` has `A` arriving as `Char`
  (`<:<[Char, Tuple2[K$, V$]]`), where the receiver's `mTables.map(…).zip(…)` is
  already broken beforehand.

- **Dependent method type `def get[P <: Phase](p: P): Option[p.State]`**
  (root cause identified in `agent/mismatch7`; not fixed). `Type::TypeMember(id)` carries
  no prefix, so at the point the signature is assembled `p.State` becomes
  `Phase`'s abstract member `State` itself, and at the call site there is no way to
  substitute `p := Phase.assignUniqueSymbols` and dealias to
  `UsedFeatures`. In slick,
  `state.get(Phase.assignUniqueSymbols).map(_.aggregate).getOrElse(true)` becomes
  `found: Any required: Boolean` (4 cases, plus cascades such as
  `value aggregate is not a member of Phase.State`).
  Fixing it requires giving `TypeMember` a prefix (nsc's
  `TypeRef(SingleType(NoPrefix, p), sym, Nil)`).

- **When the implicit clause of a jar member comes from the JVM descriptor, it stops being
  implicit** (confirmed in `agent/mismatch7`; same on main).
  `mutable.ArrayBuilder.make[E]` stays as the method type
  `(ClassTag[E])ArrayBuilder[E]`. `pickle_supply` can read the implicit flag, but
  this path never enters it (the member is already there from classfile reading, so
  `supply_from_pickle` only runs "when it was not found"), and a JVM descriptor has
  nowhere to record that a clause is implicit.
  Things supplied via the pickle, such as `Array.empty`, do work.

- **An anonymous class implementing a parent's method with a primitive type argument boxes
  twice** (noticed in `agent/mismatch7`; not fixed. Same on main).
  `new It[Int] { def next(): Int = … }` emits `boxToInteger` twice in the body of
  `next()Ljava/lang/Object;`, and `java -Xverify:all` rejects it with
  `Type 'java/lang/Integer' … is not assignable to integer`.
  With a reference type argument (`new It[String]`) it passes.

- **`PartialFunction` cannot be made a parent of `Map`** (`agent/mismatch6`).
  `Map[K, V] <: PartialFunction[K, V]` is a fact in 2.13, but
  adding that to `prelude_hier.rs` changes the traversal order of inherited members, and the
  `A` of the `toMap` above turns from `Tuple2[…]` into `Char`, regressing slick 526 → 570.
  For now the fact is only written into `Typer::function_view`, so
  `val pf: PartialFunction[String, Int] = aMap` still does not compile.

- **Sequence patterns that name `scala.collection.immutable.ArraySeq` /
  `mutable.ArraySeq` explicitly** (`case ArraySeq(a, b)`). In `agent/seqpat` we added
  `unapplySeq` to the companions of `Seq` / `Vector` / `IndexedSeq` / `Array`,
  but `ArraySeq`'s companion is not in the prelude. Matching an `ArraySeq`
  value with `case Seq(a, b)` does work (it reads by index at run time, so it does not fail
  even on the return value of `"abc".map(_.toString)`). To add it, write the JVM name in
  **both** `SEQ_FACTORY_MODULES` in `prelude_seqpat.rs` and
  `SEQPAT_SEQOPS_MODULES` in `gen.rs`.
- **`-` / `removed` / `incl` / `excl` / `filter` on `MapOps` / `SetOps` cannot be narrowed
  to the receiver's collection** (root cause identified in `agent/mismatch5`; not fixed).
  On the JVM these return **named classes** called `Map` / `Set`, so even when the typer
  narrows to `TreeMap`, codegen re-derives the Apply's result type from the post-erasure symbol,
  pushes a `Map` into a `TreeMap` field and gets a `VerifyError`.
  Because of that we restricted `erases_to_object` to only "members that return `Object` after
  erasure". It can be lifted once the Apply's own result type survives erasure.

- **Propagating the expected type to tuple components** (tried in `agent/mismatch5` and backed out).
  Typing `(new Sel, Map(s -> a))` against `(Node, Map[Sym, Int])` types the
  `Map(s -> a)` component with no expected type, giving an invariant `Map[AnonSym, Int]`.
  Adding nsc's `protoTypeArgs` (forming an estimate of the type arguments from the expected
  type before typing the arguments) made by-name parameters get passed as `() => T`, and
  slick regressed 575 → 604, so it was backed out. A version that excludes by-name
  should work.

- **`case Seq(a, b)` cannot be used** (root cause identified in `agent/mismatch4`; not fixed).
  Only the prelude's `List` has an `unapplySeq`, so `case Seq((s, _))` falls into the
  "class pattern" branch of `type_pattern`, gets no element type, and
  `Some(s)` becomes `Some[A]` (the extractor's own type parameter).
  Adding `Seq.unapplySeq` to the prelude is easy, but codegen's
  `gen_unapply_seq_bind` is **List-specific**, starting from `checkcast scala/collection/immutable/List`,
  so passing a `Vector` as a `Seq` fails at run time.
  Either a version using `SeqOps.length` / `apply(I)` or the insertion of a `toList` is required.
  Incidentally, codegen for `case List(a, b, rest @ _*)` emits
  `VerifyError: Bad type on operand stack` **even on main** (no checkcast is emitted for the
  local that bundles the elements before the starred pattern).

- **`new` on an abstract class is not diagnosed** (noticed in `agent/seqpat`; not fixed).
  We accept `new A` for `abstract class A { def n: Int }`
  (nsc gives `class A is abstract; cannot be instantiated`). Having fixed the issue where
  modifiers were dropped by the parser, `Flags::ABSTRACT` is now set correctly,
  but the check on the `new` side is still missing.

- **In overload resolution specificity, we collapse our own type parameters to their upper bound**
  (`agent/seqpat`). nsc creates skolems, whereas we substitute `bound_hi` (default
  `Any`). In shapes where the upper bound matters, such as `def f[T <: A](x: T)` and `def f(x: B)`,
  the conclusion should be able to differ from nsc. It has not shown up in slick.

- **`Seq.toArray` / `Seq.zipWithIndex` turn into erased signatures when a certain file
  is compiled alongside** (root cause identified in `agent/impltail`; not fixed).
  slick's `elementConverters.toArray` in `ProductResultConverter` stays as
  the method type `(ClassTag[B])Any`, and `cha.length` / `cha(i)` become
  cascades of it (5 cases). Compiling `ResultConverter.scala` on its own
  resolves `Seq#toArray` to the prelude's `(ClassTag[A])Array[A]`,
  yet compiling `slick/util/ConstArray.scala` **first** resolves it to
  `IterableOnceOps#toArray : (ClassTag[B])Any` (the `Seq`
  symbol is identical, and the result of `lookup_member(Seq, "toArray")`
  switches from `Seq`'s own to `IterableOnceOps`'s).
  `Array[B]` erases to `Object` in the classfile, so when a classfile-derived
  member covers the prelude's member, the result type becomes `Any`.
  This is not an implicit problem; it is a problem of member supply at class completion.

- **No checkcast is inserted on the result of `implicitly[C[T]]`** (confirmed in `agent/impltail`;
  not fixed. Same on main). `def f[T: C](…) = implicitly[C[T]].name` pushes
  `implicitly`'s return value (erased to `Object`) directly as the receiver of a `getfield`,
  so it becomes `VerifyError: Bad type on operand stack`.
  Receiving the context bound's evidence by name (`def f[T](…)(implicit c: C[T])`)
  does work.

- **`Integral[T]` / `Fractional[T]` do not become `Numeric[T]`**.
  `Numeric[T] <: Ordering[T]` is wired up in `crates/typer/src/prelude_numhier.rs`,
  but `Integral` / `Fractional` are not in the symbol table at the point the prelude is
  assembled (they are read from the jar when a source mentions the name), so their parents
  cannot be wired up in the same place.

- **Extension methods via cats syntax (`import cats.syntax.all._`)** now
  **reach real cats** as of `agent/catsyntax` (the section above).
- **The issue where the result type of a jar member becomes a bare `F`** was
  **fixed** in `agent/companionkind` (the "The companion and the class are different symbols" above).
  The adjacent gap left over there —— nested classes of a jar companion
  (`Outcome.Succeeded(_)` / `Resource.ExitCase.Errored(e)`, 6 cases) —— is written up at the end
  of that same section.
- **What is left over from reading jar classes from pickles (`agent/jarpickle`)**.
  - **Implicit search through cats' `implicits`**. The signature of `Monad[F]` now arrives
    correctly, but finding `Monad[Option]` from `import cats.implicits._`
    (walking the deep inheritance of `cats.instances.*`) does not work.
    Things like `value flatMap is not a member of F[Any]` remaining in slick's
    `BasicBackend.scala` are cats syntax extension methods, and need the same implementation.
  - **Derived implicits such as `Ref.Make[F]`**. `Ref.of[F, Int](0)` gets its signature
    through and stops at `could not find implicit value of type Make[F]`
    (derived from `MakeLowPriorityInstances#syncInstance` via `Sync[F]`).
    Note that **implicits placed directly on the companion** (`Async[IO]` =
    `cats.effect.IO.asyncForIO`) now do arrive. Three points: loading the companions in
    SLS 7.2's implicit scope before the search (`Typer::warm_implicit_scope`;
    a jar companion is a separate classfile nobody requests, so as-is the
    scope was empty), putting the pickle's `IMPLICIT` flag on methods
    (the classfile has no such bit), and not treating a class reference that has had
    **no application at all** in the pickle → Type conversion (`IO` as the argument of
    `Async[F[_]]`) as an arity error. Because `adopt_binary_class` on the whole
    companion pulls in cats-effect's transitive closure and takes minutes, we supply
    only the implicit members (`PickleSupply::supply_implicit_members`).
    The third point is allowed **only when the position demands a higher kind** (`want_arity`
    is passed to `conv_ref`). Allowing it everywhere made an `Iterable` in a plain position
    become a zero-type-argument `Iterable`, hiding the real `map`, and slick regressed
    745 → 844 errors.
  - **Even at source level, inference across higher-kinded argument clauses does not work**.
    The `a` in `F.flatMap(fa)(a => F.pure(a))` becomes `Any`. This is unrelated to jars;
    writing the same shape in source fails the same way (confirmed with `trait MyMonad[F[_]]`).
  - **The pickle writer's parameter clauses and parents**. See "Not implemented" above.
    Both are limits of reading back a jar we emitted ourselves, and they do not affect
    the `-cp <directory>` path (which reads the classfile's interfaces).
  - **Results can differ between `-cp` being a directory and being a jar**. A directory goes through
    `install_classpath` (the backend's unpickler; parents stay as `Object`), a jar goes through
    `adopt_binary_class` (`crates/pickle` plus the classfile's interfaces).
    The jar path is currently the more accurate one, and `Monadic[Option] <: Functor[Option]` fails only
    on the directory side. Unifying them means moving the directory side onto `adopt_binary_class` too.

- **`List.newBuilder` / `Vector.newBuilder` are missing from the companions**. `Builder[A, To]`
  itself is supplied from the pickle and works (`ctacc_builder` passes), but the companion's
  `newBuilder` is a polymorphic method and so is not supplied. Trying to add it by declaring
  `Builder` in the prelude ourselves hides the pickle-side `Builder` (which extends `Growable` and
  has an abstract `addOne`), so `class ListB extends Builder[...]` stops implementing `addOne`
  and gets an `AbstractMethodError` at run time. Tried and backed out.

- **slick measurements are taken after expanding the `.fm` templates**. slick keeps 7 files
  such as `GetResult` / `SetParameter` / `TupleSupport` as FreeMarker templates and
  generates them at build time. Measuring without generating them makes the 7 files that depend
  on those 7 emit errors that "scalac fails on too", so we expand them with `tests/expand_fm.py`
  and compile them together (`tests/slick_measure.sh` runs it automatically). Once those 7 are
  included, the measurement target goes 177 → 184 files and the error count rises a step (1371 → 2064).
  The number going up is not a regression; it is the measurement catching up with what is actually compiled.
  After that, the `agent/genrep` slice brought it to **2064 → 1300** (the 7 generated files went 736 → 41).
  For the breakdown and what remains, see "Until the 7 files slick generates (`.fm` templates) compile".
  The `agent/ctoraccessor` slice took it further to **1279 → 1219** (files containing errors 109 → 107;
  the 21 `tupled` cases in `CompilableFunctions.scala` and the 6 `++=` cases on `Builder` are zero).
  The `agent/mismatch2` slice then made it **1279 → 1123** (`type mismatch` 320 → 227,
  files containing errors 109 → 107). Classifying the remaining `type mismatch` mechanically gives
  "an unsolved type parameter is printed as-is" 81, "same class, only the type arguments differ" 27,
  "widened to `Any`" 14, "found and required have the same spelling" 11, and the rest are small one-offs.
  Then the `agent/tyvar` slice (undetermined type variables) gave **1059 → 1029** (files containing
  errors 105 → 104, `no matching overload` 280 → 266, `type mismatch`
  231 → 217). What went away was the shape "a polymorphic reference reaches an argument position still
  carrying its type parameters" (the ones where `Vector[A]` / `Map[K, V]` / `Set[A]` show up in `found`).
  No file newly started producing errors (in a few places one extra cascade appeared on a line that
  already had an error). The same slice **deleted** `relax_open_tparams`
  (the stopgap that collapses undetermined type parameters to `Any`; per the record in the README it had
  been the cause of three separate bugs).
  The `agent/ovl2` slice (the candidate set for overloads) took it further to **1059 → 903**, with
  files containing errors going 105 → 104.
  The `agent/mismatch3` slice gave **833 → 772** (`type mismatch` 201 → 168,
  files containing errors 102 → 100, and no file newly started producing
  errors). There were 8 root causes, and more of the "cascades that were failing upstream of it"
  disappeared than `type mismatch` itself. The mechanical classification of the remaining 168 is: one-offs 46,
  "`found` is a bare type parameter" 36 (of which the `F` cases are cats' HK signatures, discussed below),
  "a type member through a self type / `ProfileAction`" 25, "same class, only the type arguments differ" 21,
  "a tuple component cannot be solved" 11, "a collection's result type widens" 11,
  "`found` and `required` have the same spelling" 8, "the element type of `Some`/`Failure`" 6,
  and "`this` does not conform to `type Self >: this.type`" 4.

- **jar classes are read from the JVM generic signature, not from `ScalaSignature`**.
  Only when a **directory** is passed to `-cp` do we read pickles (`load_classpath` does not walk
  inside jars); classes in a jar are built by `install_java_class` from the classfile's `Signature` attribute.
  A JVM signature cannot express the application of a higher-kinded type, so cats'
  `def pure[A](a: A): F[A]` arrives as `<A:Ljava/lang/Object;>(TA;)TF;`, and
  `F.pure(v)` becomes `found: F  required: F[R]`. slick's most error-heavy files,
  `BasicBackend.scala` (54 cases) and `ConcurrencyControl.scala` (16 cases), are entirely this,
  and they account for most of the 36 remaining "bare type parameter" `type mismatch` cases. Fixing it
  means using `crates/pickle` (already a full-featured unpickler) for jar classes as well.

- **Dependent method types such as `p.State` are not substituted**. `def get[P <: Phase](p: P): Option[p.State]`
  the result arrives still as `Option[Phase.State]`, and `state.get(Phase.assignUniqueSymbols)
  .map(_.aggregate).getOrElse(true)` becomes `found: Any  required: Boolean`
  (4 occurrences). The cause is that `Type` has no variant for a prefixed type member (`p.State`).

- **`this` does not conform to `type Self >: this.type <: Node`** (4 occurrences). There is no
  conformance rule (`X <: lo ⇒ X <: Self`) for an abstract type member whose lower bound is
  `C.this.type`, so `val n: Self = if(…) this else rebuild(…)` becomes
  `found: BinaryNode  required: Node.Self`. The hard part is that the type of `this` is a plain
  class type rather than a `ThisType`, so adding the rule naively would also let
  "passing a different `Node` as `Self`" through.

- **`T` cannot be solved when a `Map[K, V]` is passed to an `Iterable[T]`**. This is a gap in
  inference rather than in the conformance check: merely passing `Map[String, Int]` to
  `def h[T](xs: Iterable[T]) = xs.size` gives `no matching overload` (`h[(String, Int)](m)` and
  `def h2(xs: Iterable[(String, Int)])` do work). The 5 occurrences of slick's
  `ConstArray.from(newDefsM.map(…))` are this.

- **JDK members of `java.lang.String` are not read on demand**. Only what the prelude declares
  (`add_string_members` in `prelude.rs` / `add_string_extra` in `prelude_text.rs` /
  the `indexOf` family in `prelude_strhier.rs`) exists, so `s.codePointAt(0)` gives
  `value codePointAt is not a member of String`. Unlike other Java classes,
  `Type::String` does carry a receiver class symbol, yet member lookup hits the prelude
  first, so it never reaches `ensure_java_loaded`.

- ~~**there is no override checking**~~ → **landed in `agent/override`** (the "conformance checking for overrides" section).
  It checks SLS 5.1.4 items 1-9 and SLS 5.2.6 (`needs to be abstract`). What remains is
  **`final` and deferred for members on the library side**: `PickleSupply` does not carry the pickle's
  `FINAL` / `DEFERRED` bits (members are created with `Flags::EMPTY`), so neither overriding a
  `final` method from the jar nor forgetting to implement an abstract member of a jar trait can be
  diagnosed. Source-derived and Java-classfile-derived cases (`classpath.rs` reads `ACC_ABSTRACT`)
  are diagnosed. The right way to close this is to add flags to `Shape`.
  The other remaining item is **"accidental override" in `class C extends A with T`**
  (scalac: `class C inherits conflicting members`). That is the rule requiring `override` when
  unrelated `A.f` and `T.f` collide; it is a different rule from 1-9, so it is not implemented.
- **implicit conversion from `Array[T]` to `Seq[T]`**. `def k(x: Array[Int]): Seq[Int] = x` compiles in scalac
  (with a deprecation warning), but here it becomes a type mismatch. The prelude has no implicit conversion
  equivalent to `Predef`'s `copyArrayToImmutableIndexedSeq` / `wrapIntArray`.
- **`Vector[T]` does not conform to `scala.collection.IndexedSeq[T]`**. The prelude's
  collection hierarchy has no edge from `immutable.Vector` (or `immutable.IndexedSeq`) to
  `collection.IndexedSeq`. Writing `immutable.IndexedSeq` works, so all that is missing
  is the edge.
- **When `F` serves as both a type parameter name and an implicit value name, the type side wins**.
  In `F.pure` of `def f[F[_]](implicit F: Sync[F]) = F.pure(x)`, the type parameter `F` is chosen
  instead of the value `F`, giving `found: F  required: F[R]`
  (slick's `BasicBackend.scala`). Name resolution does not separate terms from types.
- **The remaining type-variable gaps are on the argument side, where the argument is not yet a value**. Typing
  arguments without an expected type has not itself been changed (overload resolution needs the argument types
  first), but the undetermined type variables that come out of it are now carried around and solved in the
  `agent/tyvar` slice (the "undetermined type variables" section). What remains is when the argument's type is **not yet a value type**:
  - `Array.empty` arrives at the argument position as `(ClassTag[T])Array[T]`, i.e. still a method type
    with an implicit section left over. scalac can pass `Array.empty` to `take(a: Array[String])`, but
    here it gives `no matching overload … with arguments ((ClassTag[T])Array[T])`.
    The cause is not applying the leftover implicit section at the argument position, not the type-variable side.
    Writing `Array.empty[String]` works.
  - A function literal as a tuple element, as in `f(("x", n => n + 1))`, is
    **rejected by scalac 2.13.16 as well** (`missing parameter type` +
    `no type parameters for method apply … exist so that it can be applied to
    arguments (String, ? => ?)` + `undetermined type`). What used to be written here,
    "scalac accepts it", was wrong. The shape `f("abc", s => s.length)`, which
    determines a later argument's lambda parameter type from an earlier argument in the same section,
    is rejected by scalac too (we accept it, so this is a gap on the too-permissive side).
  - `h(new Box(Map.empty))` (`def h[A](b: Box[Map[String, A]])`) is
    **rejected by scalac too** (because `Box` is invariant).

- **Explicit type application sometimes does not propagate into the implicit argument list**.
  In `Library.Abs.column[P1](n)` (`def column[T : TypedType]`) and
  `Library.==.typed[Boolean](ch)` (the overloaded `def typed[T : ScalaBaseType]`),
  the explicitly given type argument does not reach the following implicit section, so it goes looking for
  `TypedType[P1]` / `ScalaBaseType[T]`. This is a different gap from inference from the expected type (implemented);
  it lies on the TypeApply and overload-resolution side.
- **Resolving a type member through a self type**. Inside `trait JdbcTypesComponent { self: JdbcProfile => }`,
  writing `BaseColumnType` picks, instead of the self type's `type BaseColumnType[T] = JdbcType[T] &
  BaseTypedType[T]` (the concrete alias), the abstract declaration on the linearization side,
  `type BaseColumnType[T] <: ColumnType[T] & BaseTypedType[T]`.
  As a result the evidence of `def base[U : BaseColumnType]` does not conform to `JdbcType[U]`, and the
  parent implicit section of `new MappedJdbcType[T, U] with BaseTypedType[T]` becomes
  `could not find implicit value of type JdbcType[U]` (scalac accepts it).
- **`Ordering[Null]` is not found by the search**. nsc builds
  `Ordering.ordered[Null](Predef.$conforms[Null])`, but here the identity-view fallback
  (using `A <: B` as `A => B`) is not run for the **nested** implicit arguments of
  `implicit_tree`, so `ordered` cannot be taken as a candidate. It is a pre-existing gap that
  makes `implicitly[Ordering[Null]]` fail on its own too; because parent constructors and
  argument-less `new` are now filled in, it became visible from slick's
  `new ScalaBaseType[Null]` as well.
- **When a value class (`extends AnyVal`) mixes in a universal trait**, an instance of
  `final class C(val x: Rep[Int]) extends AnyVal with Numeric[Int, Int]` yields
  a classfile that does not implement the interface
  (at run time, `IncompatibleClassChangeError`). Because the value class emits no box.
- **A line starting with `-1` right after `}` is read as a continuation of the expression**.
  The `-1` on the line immediately after `if (c) { return n }` parses as `(return n) - 1`, giving
  `value - is not a member of Nothing` (scalac breaks at the newline).
- **The parent constructor's implicit section is not filled in**.
  When `class ConstColumn[T : TT] extends TypedRep[T]` inherits
  `abstract class TypedRep[T](implicit val tpe: TT[T])`, `extends` has no argument list,
  so no witness is passed and codegen calls a `TypedRep.<init>()` that does not exist
  (at run time, `NoSuchMethodError`). It is **silently accepted**, so it is a gap that should be fixed.
  The hard part is that the tree in parent position is typed twice, so the implicit argument that was filled in
  (synthesized as an `Ident`) gets re-resolved by name on the second pass and breaks.
  ClassTag's `ClassTag.apply(classOf[T])` fallback cannot be typed in parent position either.
- **Where the `$extension` static methods of a value class are placed** differs from nsc. nsc puts the body in the
  companion `C$` and makes the class side a forwarder, whereas scala-rs emits it directly on the class side.
  Within one program the two are equivalent, but they cannot interlink with classfiles emitted by scalac.
  (Universal trait implementation, box / unbox, pattern matching, array elements and `equals` / `hashCode` were
  brought in line with nsc in `agent/valclass`.)
- **Value classes on the library side are not boxed**. The prelude models `StringOps` / `ArrayOps` as
  identity conversions such as `augmentString`, and holds positions that are "really `String`" — like the
  result of `map` — at a value class type, so boxing there would pass a `StringOps` to
  `println`. Boxing is therefore restricted to the value classes this compilation unit emits
  (`erasure::note_source_value_classes`). It is a restriction that can be lifted once the prelude's
  `StringOps` signatures match the real thing.
- **Conflating boxed types with value classes**. The prelude gives `scala.Int` the JVM name `java/lang/Integer`,
  so `java.lang.Integer` / `java.lang.Long` resolve to `scala.Int` / `scala.Long`.
  `java.lang.Integer.valueOf(3)` gives `value Integer is not a member of
  <notype>`, and `add(7L)` on `new java.util.ArrayList[java.lang.Long]` becomes a type mismatch.
  In scalac they are distinct types, so they need to be split into separate symbols.
- **`Array` invariance**. `Array[Int]` can be passed where `Array[Any]` is expected (scalac rejects it).
  Class type arguments were made to require equality in invariant position, but `Array` alone was left
  covariant. Accepting `val a: Array[AnyRef] = Array("x", "y")` requires **inference of method type
  parameters from the expected type**, which is unimplemented. Both should be fixed together.
- **The scope of Java statics**: only "not visible through an instance" is implemented (the same
  `value parseInt is not a member of Integer` as scalac). Statics are not re-hosted as real members of the
  companion object the way nsc does, so `java.lang.Integer.valueOf` is let through as a selection via the
  class symbol.
- **The ConstantValue of Java `static final` constants is not read**. `public static final
  int functionNoTable = 1;` becomes `Int` per its descriptor, so where scalac narrows the
  constant type `Int(1)` to `Byte` / `Short`
  (`val q: Short = java.sql.DatabaseMetaData.functionNoTable`), we get
  `type mismatch; found: Int  required: Short`. Reading the classfile's `ConstantValue`
  attribute and attaching `Type::Constant` fixes it. The pattern position
  (`case DatabaseMetaData.functionNoTable`) works, because `Int` constants are now allowed in the
  scrutiny of `Byte` / `Short` / `Char`.
- **Literal notation for `Long.MinValue` / `Int.MinValue`**. `-9223372036854775808L` gives
  `integer literal out of range`, and `-2147483648` gives `type mismatch; found: Long
  required: Int` (scalac folds the unary `-` into the literal). The workaround is
  `-9223372036854775807L - 1L`.
- **`unary_+`**. `+x` is not declared on any numeric type.
- **`Array(...)` varargs on the private runtime**. `Array(1, 2)` / `Array(1L, 2L)` /
  `Array(1.toByte)` all give `no matching overload for (Int)Any` under `--no-scala-library`
  (`new Array[T](n)` works). It is not a gap specific to `Byte` / `Short`; the private runtime
  simply has no `ClassTag`.

- **Temporary-directory collisions in the test harness** (`cargo test --workspace` fails intermittently).
  `tmp_dir(tag)` in `crates/cli/tests/{xsource3,imports,e2e,lang,...}.rs` builds names as
  `{tag}-{pid}-{nanos}`, but several tests use the same fixture name as their tag, and macOS's
  `SystemTime` has microsecond granularity, so under parallel execution two tests that enter at the
  same instant **share the same directory**. One test's `remove_dir_all` deletes
  the other's classfiles, producing `NoClassDefFoundError` or
  `ClassFormatError: Truncated class file`. It reproduces on main as well.
  Running each suite on its own always passes. Adding an in-process counter to `tmp_dir` fixes it
  (`crates/cli/tests/outer.rs` does exactly that).
- **Separate compilation of a trait's member class**. A single run (several files passed to one
  `compile`) works, but a different run that reads previously emitted classfiles via `-cp` gives
  `value describe is not a member of People`. Members of a member class cannot be restored from the
  pickle (a gap that predates the `$outer` work; not changed this time).
- **Rewriting `x.foo = v` into a setter method call**. For `class C { def foo: Int = …;
  def foo_=(x: Int): Unit = … }`, `c.foo = 4` would be `c.foo_=(4)` in nsc, but here it passes the type
  check and then does a `putfield` on the field, so it fails with `NoSuchFieldError: foo`.
  Only refinement types (`structural_select_lhs`) are rewritten.
- ~~**Checking the `override` modifier and override conformance**~~ → **landed in `agent/override`**.
  Both `val` and `var` are checked just like `def` (`ov_valdef_bad` / `ov_var_bad` /
  `ov_modreq_bad`). What is left is the library-side flag item above.
- **Remainder of implicit search**: unification and recursive derivation of polymorphic implicits, cutting off divergence, and nsc-equivalent specificity have landed (the "Implicit resolution" section). What remains is (a) putting `xs.toMap` on `scala.collection.Iterable` as well — pickle supply attaches its own `toMap` to concrete collections (`HashMap` / `ConstArray` …), so an inherited second one becomes an overload conflict. Right now it is declared only on `List` / `Iterator`, (b) implicits that require inference of method type parameters from the expected type (slick's `TypedType[T]` / `TypedType[P1]` are these; it is not the implicit search but the inference of `T` that is needed first), (c) the diagnostic wording is still a single line rather than nsc's multi-line form (`both … and … match expected type …`)
- **Remainder of def macro expansion**. The JVM bridge (`docs/macros.md` §2 / §7.11) has landed, and
  macro implementations are **really loaded and called**. Given `java` and scala-reflect.jar,
  `def f(): Int = macro Impl.m` expands, and the expanded program matches real scalac's
  output (`crates/cli/tests/engine.rs`). What is left: the pickle of the
  macro binding (nsc's `MACRO` flag + `@macroImpl`; §5 —
  which is why a macro def cannot be expanded from a *different run*),
  implementations that return `c.Expr[T](tree)`, tags for inferred type arguments,
  `c.prefix` / `c.enclosingPosition` / `c.typecheck` / `c.inferImplicitValue`,
  the shapes of trees that can be passed as arguments (blocks, function literals and `new` are not allowed),
  tags for types with type arguments, whitebox macros and macro bundles.
  Shapes that fall outside are **all diagnosed with a reason**.
  The tests are `crates/cli/tests/macros.rs` and `crates/cli/tests/engine.rs`
- **Remainder of quasiquote reification**. `q"..."` can lower literals / names / selections /
  applications (including curried ones) / `$x` holes / `..$xs` for one argument-list section into
  `internal.reificationSupport.Syntactic*` and execute them
  (`crates/typer/src/reify.rs`, dual-run against real scalac). Calls from a declaring class are
  done too, and Tree construction on `scala.reflect.runtime.universe` actually runs.
  All of `tq` / `pq` / `cq` and the remaining shapes of `q` (blocks / `new` / function literals /
  `if`-`else` / `match` / type ascriptions / `val` definitions / `this` / assignment / type application),
  path-dependent types such as `c.Expr[T]`, and **`Liftable`** (lifting a hole that is not a `Tree`
  into the same tree the standard instances produce) are done as well.
  What remains is the four items in `docs/macros.md` §7.8:
  (a) shapes the parser normalizes away along with the distinction nsc keeps (right-associative operators `a :: b` /
  `if` without `else` / the `_` placeholder / by-name types),
  (b) mixing `..$` with ordinary arguments (`q"f(a, ..$xs)"`), plus inference of method type
  parameters from the expected type,
  (c) quasiquotes for `class` / `def` definitions (`SyntacticClassDef` / flag conversion for
  `Modifiers`). `ShapedValue`'s whole `q"""…"""` needs this,
  (d) the body of `reify { … }` (the compiler-builtin macro that lowers an expression into an anonymous
  `TreeCreator` class). The matching materialization of `TypeTag` / `WeakTypeTag` landed in
  §7.10, and for monomorphic types the runtime results match real scalac.
  Shapes that cannot be lowered are **all diagnosed by name**
  (`unimplemented syntax: quasiquote ...` / `a hole of type X is not lifted (…)` /
  `cannot expand reify { ... }`)
  All of `tq` / `pq` / `cq` and the remaining shapes of `q` (§7.7), plus **definitions**
  (`class` / `case class` / `trait` / `object` / `def` / `val` and `var` with modifiers;
  §7.8, `crates/typer/src/reify_defs.rs`) have landed too. What remains:
  (a) mixing `..$` with ordinary arguments (`q"f(a, ..$xs)"`),
  (b) `Liftable` (when the `x` of `$x` is not a `Tree`, nsc lifts it via an implicit;
  `mapToImpl` uses this for `$rTag` / `${c.prefix}`),
  (c) `_` placeholder function literals, right-associative operators, `if` without `else`,
  by-name / varargs parameters, procedure syntax, pattern definitions, self types, early definitions,
  `type` definitions (all of them shapes the parser normalizes away along with the distinction nsc keeps),
  (d) the expander (engine) itself has landed (§7.11, the "Remainder of def macro expansion" above).
  Shapes that cannot be lowered are **all diagnosed with `unimplemented syntax: quasiquote ...`**
- **The companion of a local `case class`**. For a `case class P(a: Int)` declared in a
  method body, class `Main$P$1` is emitted, but **the companion
  `Main$P$1$` is not emitted**, so `P(1)` (the synthetic `apply`) gives
  `NoClassDefFoundError` at run time. It is a pre-existing bug that passes the type check, and
  because `agent/defquasi` made `{ case class X(…); … }` **parseable**, it is now
  reachable from a new spelling too (the bug itself is older;
  a `case class` that is not at the head of `{ … }` took the same path all along). `case object` and
  local non-`case` classes are emitted correctly
- **The scope of `import <value>._`**. The write-back used when the prefix is a value
  (`term_import_prefixes`) is carried over across compilation units. Since it is only used when a name
  resolves to a member of that class, no actual harm has been observed, but
  it should really be pushed / popped together with the scope
- **leftover pickle holes** (this is not a complete nsc pickle): MACRO and late/anti flags were **not needed for scalac 2.13.16 to typecheck what we already emit (the `separate_lib` pickle)**, so they are not implemented. `type T = Int` is written as nsc's **ALIASsym** (tag 5). The 2.13 PickleFormat has **no ALIAStpe tag**. Reordering named annotation arguments into ctor order is **unnecessary**: scalac 2.13.16 typechecks `@Ann2(b = 2, a = "ok")` from the same positional pickle as `#29`/`#30` (the RHS order in the source). nsc itself emits a warning when it converts a named annot into a block. The Constant of `@Ann(foo = 1)` and the TREE of `@Ann(foo = this.x)` / `@Ann(foo = bar)` are written as positional arguments just as nsc does. **Why JAVA is not put on EXTREF**: PickleFormat's `EXTref` / `EXTMODCLASSref` carry only `name_Ref [owner_Ref]` and have no flags field. An extra Nat would be mistaken by scalac for the owner. `java.lang.Object` / `String` and the like are completed from the Java classfiles on the classpath, and JAVA is attached there. For local CLASSsym (when we pickle a class the prelude `mark_java`-ed ourselves) JAVA is already emitted. We do not claim this is a full pickle
- The rest of **StringOps** (everything other than `++` / `lengthIs` / `sizeIs` / `flatMap` / `iterator` / `sizeCompare` / `knownSize` / `appendedAll` / `prependedAll` / `>` / `>=` / `<=` / `compare` / `lengthCompare` / `patch(Int, String, Int)` / `<` / `map` (`Char => Char`) / `:+` / `+:` / `foldRight` / `toByteOption` / `toShortOption` / `toFloatOption` / `grouped` / `foldLeft` / `toByte` / `toShort` / `toFloat` / `toLongOption` / `toDoubleOption` / `find` / `foreach` / `toBoolean` / `toBooleanOption` / `dropWhile` / `takeWhile` / `nonEmpty` / `headOption` / `lastOption` / `filterNot` / `indices` / `r` / `sorted` / `toArray` / `copyToArray` / `partition` / `exists` / `forall` / `splitAt` / `updated` / `count` / `span` / `diff` / `intersect` / `split(String)` / `filter` / `reverseIterator`)
- The rest of **ArrayOps** (`lengthIs` / `sizeIs` / `indexOf` / `copyToArray` / `iterator` / `zipWithIndex` / `knownSize` / `sizeCompare` / `filterNot` / `headOption` / `lastOption` / `partition` / `splitAt` / `span` / `find` / `contains` / `distinct` / `takeRight` / `dropRight` / `takeWhile` / `indices` / `lengthCompare` / `last` / `init` / `reverse` / `size` / `isEmpty` / `nonEmpty` / `scanLeft` / `count` / `forall` / `foldLeft` / `fold` / `foldRight` / `drop` / `dropWhile` / `exists` / `take` / `collect` / `zip` / `filter` / `slice` / the 3-argument `flatMap` / the 4-argument Array→Iterable `flatMap` and the primitive wrappers / `genericArrayOps`'s `head`/`map`/`foreach`/`tail` are all in place. Other methods remain. `reduce` does not exist on 2.13.16's ArrayOps)
- The other mutable collections (everything other than `ArrayDeque` / `LinkedHashMap` / `LinkedHashSet` / `HashMap` / `HashSet` / `ArrayBuffer` / `ListBuffer`) and the other immutable ones (everything other than `BitSet` / `SortedMap` / `TreeMap` / `SortedSet` / `TreeSet` / `Set` / `Map` / `Vector`). `scala.collection.View`'s `List.view` / `map` / `toList` and `View.fill` / `View.iterate` have landed (the other Views have not). `scala.util.control.Breaks`'s `breakable` / `break` / `tryBreakable`+`catchBreak` have landed (the other control constructs have not). `scala.math.BigInt` / `BigDecimal`'s `apply(Int)` / `apply(String)` / `+` / `*` / `int2bigInt` have landed (the rest of math has not). `scala.util.chaining`'s `pipe` / `tap` have landed. `scala.util.Using.resource` / `Using.apply` / `Using.Manager` / `Using.resources` (2-4 arguments) have landed (the rest of Using has not)
- **Remainder of automatic symbol supply from pickles**: the reader, signature restoration, linearization and
  the hookup into type checking all work; over 60 members of `List` / `Option` / `Map` / `Set` / `Vector` / `Range` /
  `Iterator` (including operators and companion members) work with no hand-written prelude entries, and
  the runtime results match scalac 2.13.16. What remains is
  (a) **rebuilding classes that are already in the symbol table** (`scala/collection/Seq` has no type parameters,
  so `diff` / `intersect` / `union` / `indexOfSlice` / `containsSlice` cannot be supplied.
  Retrofitting broke the hand-written members, so that route is not taken),
  (b) weak subtyping because stubs are given no parent chain,
  (c) a mismatch in the **getter convention for default arguments** (a fix on the `check.rs` side is needed),
  (d) extension-method paths such as `String.format` and the Java loader path for `scala.io.Source`,
  (e) type inference originating in lambdas (`reduceOption`, inline `collect { case … }`).
- **Remainder of type aliases in a jar's package object**: the aliases in `scala` / `cats.effect` resolve, but
  those whose right-hand side is a **class nested in an object** (`type ParallelF[F[_], A] =
  cats.effect.kernel.Par.ParallelF[F, A]`) still cannot be restored.
  `install_classpath` grabs the simple name of a trait that has a companion as the module class's JVM name
  first (`Outcome` → `cats/effect/kernel/Outcome$`, with 0 type parameters), which
  `resolve_dotted_class` fixes by "re-reading the classfile the path names", but
  shapes where the middle of the path is an object, such as `Par.ParallelF`, still do not work.
  Aliases that cannot be restored are not registered, and are diagnosed with a reason at the point of use.
- **Inference for slick's `Ref[F, ExecState]`**: the alias itself now resolves, but
  `Ref[F, ExecState]` is matched against `Ref[Any, ExecState]` (the HK class type parameter `F`
  collapses to `Any`). This is a gap on the type-argument inference side, not the alias side.
  Details are in the "Automatic symbol supply from ScalaSignature" section
- **Remainder of `Either` / `Try` / `Option`**: `Either`'s `joinLeft` / `joinRight` / `flatten` / `toTry` / `cond` (the ones that require `<:<`, plus the companion), `LeftProjection`'s `filter`, `Try`'s `flatten`, `Option`'s `orNull` / `unzip` / `unzip3` / `iterator` / `when` / `unless` / `empty` / `apply` (companion). **2.13's `Either` has no `withFilter`**, so a guard in a `for` stays a compile error exactly as in nsc (use `filterOrElse`). The private runtime has neither `Either` nor `Try`, so it just diagnoses
- **`java.lang` exceptions** go as far as `ArithmeticException` / `ClassCastException` / `IllegalArgumentException` / `IllegalStateException` / `IndexOutOfBoundsException` / `NullPointerException` / `NumberFormatException` / `UnsupportedOperationException`, plus the `()` / `(String)` constructors of `Throwable` / `Exception` / `RuntimeException` and `getMessage`. Other JDK exceptions and methods are not there yet
- The rest of **`List`**: `flatten` (needs the implicit `Predef.$conforms` resolution for `A => IterableOnce[B]`), `toBuffer` / `toIndexedSeq` (`toMap` has landed), the `*Option` family other than `sortBy` (`maxOption` / `minOption` / `reduceOption`), and `patch` / `diff` / `intersect` / `unzip` / `partitionMap` / `tails` / `inits` / `corresponds` / `segmentLength` / `indexWhere` / `lastIndexWhere` / `zipAll` / `padTo` / `mapConserve` / `tapEach` / `sameElements` are all missing. The implicit `Ordering` instances are only `Int` / `Char` / `String` / `Long` / `Boolean` (`Double` requires distinguishing 2.13's `Ordering.Double.TotalOrdering` from `DeprecatedDoubleOrdering`). `Numeric` is only `Int` / `Long` / `Double`. Passing an inline `PartialFunction` literal directly, as in `xs.collect { case … }`, is unsupported by the typer (the same as ArrayOps; pass a type-ascribed `val pf: PartialFunction[A, B]`). On the private runtime side, everything beyond `map` / `flatMap` / `foreach` / `withFilter` and the core above is missing (there is no `Function2` classfile, so the `foldLeft` family cannot be emitted)
- The other mutable collections (everything other than `ArrayDeque` / `LinkedHashMap` / `LinkedHashSet` / `HashMap` / `HashSet` / `ArrayBuffer` / `ListBuffer` / `StringBuilder`) and the other immutable ones (everything other than `BitSet` / `SortedMap` / `TreeMap` / `SortedSet` / `TreeSet` / `Set` / `Map` / `Vector`). `scala.collection.View`'s `List.view` / `map` / `toList` and `View.fill` / `View.iterate` have landed (the other Views have not). `scala.util.control.Breaks`'s `breakable` / `break` / `tryBreakable`+`catchBreak` have landed (the other control constructs have not). `scala.math.BigInt` / `BigDecimal`'s `apply(Int)` / `apply(String)` / `+` / `*` / `int2bigInt` have landed (the rest of math has not). `scala.util.chaining`'s `pipe` / `tap` have landed. `scala.util.Using.resource` / `Using.apply` / `Using.Manager` / `Using.resources` (2-4 arguments) have landed (the rest of Using has not)
- **The plain methods of `java.lang.String`** (added `trim` / `substring` (1 and 2 arguments) / `lastIndexOf` / `replace(Char,Char)` / `replace(CharSequence,CharSequence)` / `contains(String)` / `equalsIgnoreCase` / `matches` / `strip` / `repeat` / `compareTo`; they are not duplicated with the existing `startsWith` / `endsWith` / `indexOf` / `split` / `charAt` / `concat`, nor with `toUpperCase` / `toLowerCase` / `isEmpty` which go through StringOps). `chars()` / `codePoints()` (which return `java.util.stream.IntStream`) are unsupported, since there is no infrastructure for Stream types
- **`scala.collection.mutable.StringBuilder`** (added bare `StringBuilder`, i.e. the `scala.StringBuilder` alias. All the primitive `append` overloads plus `String` and `Any`, `+=` (`Char`), `++=` (`String`), `insert`, `deleteCharAt`, `setLength`, `reverse`, `clear`, `isEmpty` / `nonEmpty`, `length`, `result`, `charAt`, `apply`, and the `(Int)`/`(String)` constructors. `reverse` comes from `IndexedSeqOps` and is erased, so it gets a checkcast)
- **The collection-style members of `Range`** (added `withFilter` (required for guards in for-comprehensions) / `filter` / `filterNot` / `map` / `flatMap` / `foldLeft` / `foldRight` / `sum` / `product` / `min` / `max` / `reverse` / `toList` / `toArray` / `toVector` / `zipWithIndex` / `exists` / `forall` / `count` / `take` / `drop` / `takeWhile` / `dropWhile` / `head` / `last` / `isEmpty` / `nonEmpty` / `size` / `contains` / `by` / `splitAt` / `slice` / `takeRight` / `dropRight`. `sum` / `min` / `max` are called directly on `Range`'s own `int`-returning overloads by passing `Numeric$IntIsIntegral$` / `Ordering$Int$`. `filter` / `filterNot` / `flatMap` / `zipWithIndex` / `toArray` have only a single overload erased to `Object`, so they go through a checkcast / `ClassTag`)
- **The functions on the `scala.math` package object** (added `abs` / `max` / `min` / `signum` (`Int`/`Long`/`Float`/`Double`) / `pow` / `sqrt` / `cbrt` / `floor` / `ceil` / `round` / `random` / `exp` / `log`. The implementation is an `invokestatic` on the static forwarder class `scala.math.package`)
- **Gaps in numeric enrichment** (added `RichInt`/`RichLong`.`toBinaryString`/`toHexString`/`toOctalString`/`sign`, `RichDouble`.`isNaN`/`isInfinity`/`round`/`floor`/`ceil`/`sign`, `RichChar`.`isLetter`/`isLetterOrDigit`/`isUpper`/`isLower`/`isWhitespace`/`toUpper`/`toLower`. `sign`/`round`/`floor`/`ceil` have no `$extension` static, so they delegate to `java.lang.Integer/Long.signum` and `java.lang.Math`. `RichInt`/`RichLong`/`RichDouble`/`RichChar`/`RichByte`/`RichShort`.`compare` have no `$extension` either and would need real-instantiation codegen like `RichBoolean.compare`, so they remain unsupported — only `RichBoolean.compare` is implemented, by reusing the existing codegen)
- The other mutable collections (everything other than `ArrayDeque` / `LinkedHashMap` / `LinkedHashSet` / `HashMap` / `HashSet` / `ArrayBuffer` / `ListBuffer` / **the new `mutable.Map` / `mutable.Set`**) and the other immutable ones (everything other than `BitSet` / `SortedMap` / `TreeMap` / `SortedSet` / `TreeSet` / `Set` / `Map` / `Vector`). `scala.collection.View`'s `List.view` / `map` / `toList` and `View.fill` / `View.iterate` have landed (the other Views have not). `scala.util.control.Breaks`'s `breakable` / `break` / `tryBreakable`+`catchBreak` have landed (the other control constructs have not). `scala.math.BigInt` / `BigDecimal`'s `apply(Int)` / `apply(String)` / `+` / `*` / `int2bigInt` have landed (the rest of math has not). `scala.util.chaining`'s `pipe` / `tap` have landed. `scala.util.Using.resource` / `Using.apply` / `Using.Manager` / `Using.resources` (2-4 arguments) have landed (the rest of Using has not)
- Known gaps found in the **collections slice** (`ArrayBuffer` / `ListBuffer` / the new `mutable.Map` / `mutable.Set` / `immutable.Map` / `immutable.Set` / `Vector` / `Tuple2.swap`). In every case the feature itself was left unimplemented so that it does not silently produce broken behaviour (the members in question were not added to the prelude):
  - **`++` / `concat` on `immutable.Map`**: `scala.collection.immutable.Map` does not override `iterableFactory` (only `mapFactory`), and the inherited default implementation of `IterableOps.++`/`concat` builds via `iterableFactory`, so measuring against `scala-library-2.13.16.jar` confirmed that `Map(...) ++ Map(...)` returns a `List` (`::`) instead of a `Map` and gives a `ClassCastException` (the same for the `Map1`-`Map4` specialized classes and for the `HashMap`-backed case). `immutable.Set.++` correctly returns a `Set` through the same path (measured, covered by `coll_immutableset1.scala`), so this is recorded as an asymmetric gap
  - **Method type parameter inference for `MapView.mapValues[W]`**: when `W` can only be determined from the lambda's return type (an expression such as `v => v * 2`), the current inference runs before the lambda body is typed and `W` is left unresolved. Writing `mapValues[Int](...)` explicitly at the call site works (`coll_map_view1.scala` uses that shape)
  - **The parameter type of a single-argument lambda that destructures a Tuple2**: `map.foreach(p => ...)` (taking the whole `p: (K, V)`) works, but for things like `MapView.foreach`, where the expected type is a concrete `Tuple2[K, V]` (type arguments already instantiated), `p`'s type can wrongly collapse to just `K` (`p => p._2` gives `_2 is not a member of K`). As a workaround, in cases like `MapView`, use `toList` / `mkString` instead of `foreach`
  - **Tuple pattern destructuring with `case (k, v) => ...`** (`Map.foreach { case (k, v) => ... }`) is a type error on both immutable and mutable `Map` (a pre-existing bug that exists for the existing `Map` too; it is not caused by this slice). The workaround is a lambda using `p._1` / `p._2`
  - **`toArray` on `ArrayBuffer` / `ListBuffer`**: deferred, because there is currently no mechanism to derive the implicit `ClassTag[A]` generically from a method type parameter (only special cases with a fixed element type, like `StringOps.toArray`, exist)
- The only thing that declares a lower bound so far is `List.::`. `:::` / `+:` / `:+` / `concat` / `++` / `appended` / `prepended` / `updated` / `max` / `min` / `sum` / `product` / `Option.getOrElse` / `Either.getOrElse` **do not even have members in the prelude yet**, so declaring the same `[B >: A]` when they are added will put them straight onto this inference path (`crates/typer/src/prelude_lowbound.rs`)
- Member selection through a type parameter with an upper bound (`def f[A <: Named](x: A) = x.name`) is unimplemented. Checking the bound and conformance to a position expecting `Named` is as far as this slice goes. Unlike nsc, erasure always turns type parameters into `Object`
- The rest of **ArrayOps** (on top of `lengthIs` / `sizeIs` / `indexOf` / `copyToArray` / `iterator` / `zipWithIndex` / `knownSize` / `sizeCompare` / `filterNot` / `headOption` / `lastOption` / `partition` / `splitAt` / `span` / `find` / `contains` / `distinct` / `takeRight` / `dropRight` / `takeWhile` / `indices` / `lengthCompare` / `last` / `init` / `reverse` / `size` / `isEmpty` / `nonEmpty` / `scanLeft` / `count` / `forall` / `foldLeft` / `fold` / `foldRight` / `drop` / `dropWhile` / `exists` / `take` / `collect` / `zip` / `filter` / `slice` / the 3-argument `flatMap` / the 4-argument Array→Iterable `flatMap` and the primitive wrappers / `genericArrayOps`'s `head`/`map`/`foreach`/`tail`, the **conversion and aggregation** methods (`toList` / `toSeq` / `toIndexedSeq` / `toSet` / `toVector` / `toBuffer` / `groupBy` / `sortBy` / `sorted` / `sortWith` / `sum` / `product` / `min` / `max` / `minBy` / `maxBy` / `mkString` / `reduce` / `reduceLeft` / `indexWhere` / `lastIndexOf` / `patch` / `updated` / `appended` / `prepended` / `concat` / `++`) are in place too. Other methods (`sliding` / `grouped` / `distinctBy` / `startsWith` / `endsWith` / `padTo` / `transpose` / `unzip` / `unzip3` / `intersect` / `diff` / `combinations` / `permutations` and so on) are missing)
- The other mutable collections (everything other than `ArrayDeque` / `LinkedHashMap` / `LinkedHashSet` / `HashMap` / `HashSet` / `ArrayBuffer` / `ListBuffer`) and the other immutable ones (everything other than `BitSet` / `SortedMap` / `TreeMap` / `SortedSet` / `TreeSet` / `Set` / `Map` / `Vector`). `scala.collection.View`'s `List.view` / `map` / `toList` and `View.fill` / `View.iterate`, and **`scala.collection.MapView`** (`Map.view` / `keys` / `values` / `filterKeys` / `mapValues` / `toMap` / `toList` / `toSeq` / `size` / `isEmpty` / `foreach`) have landed (the other View/MapView members have not). `scala.util.control.Breaks`'s `breakable` / `break` / `tryBreakable`+`catchBreak` have landed (the other control constructs have not). `scala.math.BigInt` / `BigDecimal`'s `apply(Int)` / `apply(String)` / `+` / `*` / `int2bigInt` have landed (the rest of math has not). `scala.util.chaining`'s `pipe` / `tap` have landed. `scala.util.Using.resource` / `Using.apply` / `Using.Manager` / `Using.resources` (2-4 arguments) have landed (the rest of Using has not)
- **Remainder of imports**: (a) **type aliases living in a jar's package object** (`type NoSuchElementException = java.util.NoSuchElementException` in `scala/package$`, cats' `type Eq[A] = cats.kernel.Eq[A]`, and so on) do not appear in the classfile and exist only in the pickle, so they are not supplied yet. The alias itself is visible as a name, but it has no type parameters, so using it gives `does not take type parameters`. (b) **An import whose prefix is a `val` written earlier in the same template** (`object O { val h = new H; import h.Inner._ }`) becomes `<notype>`, because the import runs before the `val` is typed. A `val` placed in a different object (`import O.h.Inner._`) works. (c) Wildcard imports do not enumerate the package's entries but read one class at a time as names are demanded, so **there is no ambiguity check for a same-named class elsewhere**
- **Remainder of `-Xsource:3`**: what is implemented is only the `?` wildcard / the `&` intersection type / the varargs pattern `case Cast(ch*)` / the `*` wildcard import / the `as` renaming import. `|` union types / `enum` / `given` / `using` / `extension` / trait parameters are not in (`given` / `using` are not 2.13 syntax, so they are out of scope). `-Xsource-features:<feature>` is unimplemented as well
- **In the cake-pattern slice**, errors across 177 files went **2,901 → 2,581** and the number of files containing errors went **116 → 114** (`not found: type Table`, 34 occurrences, and `not found: type Sequence`, 17 occurrences, went to 0; `no matching overload for constructor` went 42 → 26). The remaining `not found: type Ref` / `Async` are cats-effect package-object aliases and a separate matter
- **What is left in slick's type checking**: with import resolution, errors across slick's 177 files went **13,245 → 7,727** (`tests/slick_measure.sh`). What remains from imports is just 4 occurrences — `slick.util.TupleSupport` / `ProductWrapper` / `slick.jdbc.GetResult`, which are generated from `.fm` templates at build time; they do not exist in the source set, so scalac fails on them the same way. The top remaining category is `does not take type parameters` (142), which is type inference from lambda bodies, a different area from imports. **In the named-arguments slice**, errors across 177 files went **6,504 → 6,300** and `unimplemented syntax: named arguments` went **43 → 1** (the one left is `m.Column(name = …, …)` in `slick/jdbc/JdbcModelBuilder.scala` against a case class on the `-cp`, because classpath symbols carry no parameter names)
- **What is left in parsing slick**: errors across slick's own 176 files went **23 → 11** (with `-Xsource:3`). What remains is **only the 2 def macros** at `ShapedValue.scala:21` and `TableQuery.scala:36`; excluding those two files, parse errors are **0**. `try e catch h` / `case Cast(ch*)` / `super.T` in type position were all killed in this slice
- **The `agent/smallgaps` slice**: errors across 177 files went **2,901 → 2,560** (`files_with_errors` went **116 → 115**). Three of the four items had their root causes fixed:
  - Placement validation for `@inline` / `@noinline` (11 occurrences): real scalac does not validate placement at all (`crates/typer/src/check.rs::check_stored_annotations`), so the validation itself was removed.
  - `value length/varying is not a member of FieldSymbol` (23 occurrences) and `value desc is not a member of Direction` (13 occurrences) were not a cascade but three independent root bugs: (a) when `qualified_type_owner` (`check.rs`) resolved the `Foo` of a `Foo.Bar` type path, and both a same-named case class and its companion module were candidates, the winner was decided by declaration order (fixed to prefer the companion), (b) for a case class with multiple argument lists (`case class F(a: A)(b: B, c: C)`), the companion's `apply`/`unapply` only looked at the first argument list, so currying was broken (`finish_case_apply`), (c) the module `<init>` codegen for `object X extends Y(args)` always called the no-argument super constructor, causing `NoSuchMethodError` (`crates/backend/src/gen.rs::emit_module_init`).
  - `value getOrElse is not a member of Any` (16 occurrences) was a cascade with two root causes: (a) the prelude declaration of `Option.flatMap` reused the class's own type parameter `A` and so was not polymorphic (`crates/typer/src/prelude_sgap.rs::fix_option_flat_map`), (b) the `lub` of the branches of `if`/`else` (and likewise `match`) only did a structural subtype check without walking parents, and was asymmetric on top of that (`SymbolTable::lub` was extended to search the parent chain and to be symmetric on both sides). A pre-existing bug found along the way — `None` had `parents` set on the module itself rather than on the companion **module class** — was fixed as well.
  - `value apply is not a member of Iterable` (15 occurrences) was a prelude gap, handled with the same pattern as `List` / `Seq` (in the real library the companion `apply` is inherited from `IterableFactory$Delegate` and is invisible in the pickle) (`add_iterable_apply`, restricted to the library ABI, with codegen added in `crates/backend/src/gen.rs`).
  - Custom string interpolators (`value q/tq/pq is not a member of StringContext`, 14 occurrences, in the single method `mapToImpl` of `ShapedValue.scala`) were initially assumed to be the `implicit class` pattern, but they turned out to be **quasiquotes** (`q"..."` / `tq"..."`) from `scala.reflect.macros` (`scala-reflect.jar`). **The diagnostic was corrected in the `agent/quasi` slice** (the previous wording was wrong: `q` is a member of `Quasiquotes.Quasiquote`). The contents are now actually parsed by scala-rs's parser, and measurement showed that all 14 occurrences do parse (`unimplemented syntax`: 0 occurrences). What remains is reification and the reflect ABI, enumerated in `docs/macros.md` §7.3.
  - Found along the way but not fixed: overriding a parent's abstract method with a covariant return type, as in `case object X extends Y(...) { override def m: MoreSpecific = ... }`, gives `AbstractMethodError` (the bridge method is not generated). It was hit while building the fixture, so `tests/fixtures/sgap.scala` avoids that pattern. Recorded as a separate open item.
- **Remainder of `super.T` in type position**: paths to a type member of the parent class work, but when a nested type with **the same name as the parent's** is defined, as in `trait Mid { trait Impl extends super.Impl }`, resolution of the inherited member at the mix-in site can pick the parent's one (this is not a gap in resolving `super`, but on the inheritance side for same-named nested types)
- **How to discard a polymorphic method instantiated at `Unit`**: only those that return `(Object)Object` on the JVM, such as `PartialFunction[A, Unit].apply`, get a `pop` in statement position. The condition is deliberately narrow so that it does not overlap with intrinsics that the emitter already discards, such as `Breaks.catchBreak` / `Using.resource` (`unit_call_leaves_ref`)
- **The `agent/overloadshadow` slice** (reading another class makes an existing overload set disappear): errors across 177 files went **1,707 → 1,678** (`files_with_errors` stayed at **111**). Three root causes were stacked: (a) `PickleSupply::complete` returned without looking at the companion as soon as it could supply even one thing on the class side (so the answer changed based on **unrelated global state**, namely whether `java.math.MathContext` had been loaded), (b) `check.rs::resolve_overload` re-fetches the candidate symbols of a `Type::Overload` from the owner of `fun.sym`, so one whole side of a set spanning a class and its companion is dropped, (c) once `apply(MathContext)` is on the class side, subsequent `BigDecimal(...)` calls stop as soon as `lookup_member` finds it and never reach pickle completion. (a) was fixed by merging, (b) by `Check::overload_groups` (remembering only the sets that would be lost in the re-fetch), and (c) by `Check::widen_with_companion` (**only right before emitting an error**, widening the selection on a class name in term position with the companion's members and re-resolving once). Along with that, `scala.math.BigDecimal.apply(java.math.BigDecimal)` (used to turn JDBC results into Scala values) was pinned into the prelude (`crates/typer/src/prelude_oshadow.rs`, `library_abi` only). Left open: slick's `value getOrElse is not a member of Product` (16 occurrences) has nothing to do with BigDecimal — it is a different bug where the `lub` of `if (c) None else Some(x)` does not become `Option[X]` but collapses to `Product` (it shows up the same way for `Boolean` / `Blob` / `Byte` …). `new ScalaNumericType[BigDecimal](BigDecimal.apply)`, which eta-expands `BigDecimal.apply` and passes it as `(Double) => BigDecimal`, is unsupported because eta expansion of an overload cannot be selected by the expected type
- **The `agent/quasi` slice** (groundwork for quasiquotes and the reflect ABI): errors across slick's 184 files went **1,059 → 1,050** (`files_with_errors` went **105 → 104**). The numbers are small because the point of this slice was "replacing wrong diagnostics with right ones" and "closing the gaps on the road to the reflect ABI". The gaps closed were: **nested classes** that the pickle points at (`Names.TermNameExtractor` = `Names$TermNameExtractor` — and nested classfiles do not carry a `ScalaSignature`), **traits with no parent in the bytecode** (`Universe` is an abstract class, so the classfile of `trait JavaUniverse extends Universe` has `interfaces: 0`), **abstract type members** (`type Tree >: Null <: TreeApi` — nearly the whole vocabulary of the reflect API), **inserting `apply` on the result of an argument-less `def`** (a general omission, not specific to reflect), **codegen for members of a package object** (a pre-existing bug where `scala.math.Pi` gave a `VerifyError`), and **`import <value>._`**. Details and open items are in `docs/macros.md` §7.2 / §7.3
- **The second slice `agent/reify2`** (the remaining shapes of reification): with scala-reflect.jar added to the `-cp`, errors across slick's 184 files went **257 → 255**. The numbers barely move because **the same lines now fail for different reasons**; within the quasiquote group, `unimplemented syntax: quasiquote …` (shape not supported) went **10 → 4**, `cannot expand quasiquote …` went **1 → 0**, and the total errors in `TableQuery.scala` went **11 → 6**. The 8 type ascriptions in `ShapedValue.mapToImpl` now work as shapes, and the reason they currently fail has changed to `$uTag` / `$rTag` being `WeakTypeTag` and not `Tree` (i.e. **`Liftable`** is required). The shapes implemented are all of `tq` / `pq` / `cq` and, for `q`, type ascriptions / eta expansion / type application / blocks and `val` / `new` / `match` / partial functions / function literals / `this` / assignment / `if`-`else` / tuples / encoding of operator names, all read off real scalac 2.13.16's `-Ymacro-debug-lite` and cross-checked down to `showRaw`. General gaps fixed on the way: **inserting `apply` into an overload set** (`val Ident: IdentExtractor` and `def Ident(String)`), **a term selection being eaten by a same-named type member** (`u.Modifiers(flags)` becoming `<notype>`), **the `count` of `invokeinterface` not being the slot count** (a `VerifyError` with `long` arguments), and **no erasure-adaptation `checkcast` on an argument of abstract type member type** (`Names$TermNameApi` → `Names$NameApi`). Details and open items are in `docs/macros.md` §7.7
- **`@specialized` codegen** is not started in this slice
- **What is left in the overload / method application slice**: errors across slick's 177 files went **2,901 → 2,539** (`tests/slick_measure.sh`; files containing errors went 116 → 115). `no matching overload for (Type, Any, Boolean)LiteralNode` / `(#N*)(TypedType[T])Rep[T]` / `not found: extractor ==` / `type arg is not a member of OptionMapperDSL$.arg[B1, P1]` are gone. The top remaining ones are implicit search (`could not find implicit value of type TypedType[BR]` and the like) and the cascade from types that do not exist because they come from `.fm` templates (`Table` / `Sequence` / `Ref`). `no matching overload for (String)String` works in a minimal reproduction, so it is a cascade from some other gap
- Higher-kinded `F[_] <% …` gives `takes type parameters` just as in nsc (`F[_]: C` is accepted by nsc, so it is implemented. The old description in the README was wrong and has been corrected to match measurement)
- The rest of placeholders (fully reproducing deeper nesting. The shapes needed for unary / Function2 / typed `_ : T` are as far as this slice goes)
- **Derivation of implicits** (the shape where an implicit def that itself takes implicit parameters, such as `implicit def optShow[A](implicit s: Show[A]): Show[Option[A]]`, is resolved recursively with unification of the type parameters). `implicit_provides` currently only considers implicits with an empty parameter list as candidates, so `Show[Option[Int]]` gives `no implicit`
- **StackMapTable for locals declared in a `while` body** (`while (c) { val s = …; … }` makes the frame at the top of the loop include that slot, giving `VerifyError: Instruction type does not match stack map`. It has nothing to do with anonymous classes; binding the `val` outside the loop works)
- **The body of `scala.Product`** (case classes / case objects do have `productPrefix` / `productArity`, but `Product` is not attached as a parent, so there is no `productElement` / `productIterator` / `productElementNames`, and they cannot be passed where `(x: Product)` is expected)
- **Among optional constructor arguments, defaults that refer to a preceding ctor parameter** (`class C(x: Int, y: Int = x + 1)`). Simple literal / `null` defaults (`class C(x: Int, y: Int = 5)`, or slick's `SlickException(msg, parent: Throwable = null)`) do work
- **Remainder of named arguments**: (a) **prelude / classpath methods carry no parameter names**, so `List(1,2,3).mkString(sep = "-")` and `copy(name = …)` on a case class from a jar or the `-cp` give `unimplemented syntax: named arguments (method parameters not resolved)` (neither the path that reads parameter names from scala-library's pickle nor naming in the hand-written prelude signatures is implemented. Methods and classes in the same compilation unit all work). (b) ~~**a constructor with multiple argument lists**, `class C(a: Int)(b: Int)`, does not even support `new C(1)(2)` itself, let alone named arguments~~ → `agent/tail4` implemented both `new C(1)(2)` and `new C(1)(c = 3, b = 2)` (`tests/fixtures/t4_curried_new.scala`). (c) Overloads with identical names and types differing only in order (`h(s: String, n: Int)` and `h(n: Int, s: String)`) are `ambiguous reference to overloaded definition` in nsc, but here the one declared first is silently chosen
- **`x == null` (reference type) under `--no-scala-library`** (it does not go through `scala.runtime.BoxesRunTime.equals`, so if `x` really is `null` the `invokevirtual` of `Object.equals` throws `NullPointerException`. Under `--scala-library` it works correctly)
- **The scope of the lazy completer**: definitions seen only by the namer (forward references from another template) are completed with a scope reassembled from the members of the owner chain. `import`s at the top of the file are not in place until the typer processes them, so a forward reference to a definition that uses an imported name on its right-hand side gets no type and stays `<notype>` (the diagnostic is still emitted; it is not silently accepted)
- **Codegen for reading the outer instance from a trait's member class** (`trait T { def x = 1; class Inner { def y = x } }`).
  `enclosing_instance` does not pass `$outer` to a trait's member class, so `x` ends up as a checkcast of `this` to
  `T` and gives a `ClassCastException` at run time. Reading a self-type alias (`self`) from an
  inner class goes down the same path. The type check passes (matching scalac), but an implementation that passes an
  interface-typed `$outer` through the constructor, the way nsc does, is needed. Using `self` inside a trait's **own methods** works correctly
- **Type aliases in a jar's package object** (cats-effect's `cats.effect.Ref` / `Async`, etc.).
  `import cats.effect.{Async, Ref, Resource}` cannot be resolved because it exists only in the package object's pickle,
  so `not found: type Ref` remains in slick's `slick/basic/BasicBackend.scala`.
  Pointing at the real class directly, as in `import cats.effect.kernel.Ref`, works (the same gap as "Remainder of imports" (a))
- **Codegen for reading a trait's `val` / `lazy val` from a subclass** (`IncompatibleClassChangeError: Found interface T, but class was expected`. A separate matter that predates lazysig. The fixture works around it by using a trait `def`)
- **What is left in the `agent/mutcoll` slice**: (a) **the companion of `mutable.Buffer` depends on the order of references** (a pre-existing bug that predates this slice; it reproduces the same way with `prelude_mutcoll` removed). If `mutable.Buffer(1, 2, 3)` / `mutable.Buffer[Int]()` are written first, then `Buffer.empty[Int]` works too, but if `Buffer.empty[Int]` is the **first** mention of `Buffer`, the type check passes and it fails at run time with `RuntimeException: select Buffer`; and if `Buffer` is used **as a type** earlier in the same compilation unit, it gives `value apply is not a member of Buffer$` (an order dependence in the path that completes `object Buffer extends SeqFactory.Delegate[Buffer]` from the pickle; the companion handling around `find_or_stub_java_class` is another slice's job). Going through `ArrayBuffer` / `ListBuffer` works. (b) The `Ordering` for `mutable.PriorityQueue` only covers the ones whose implicit value is in scope (`Int` / `Long` / `Double` / `String` / `Boolean` and the other things the prelude has). (c) `ArraySeq` goes as far as `apply` / `update` / `length` / `size` / `toList` / `mkString`; specialized subclasses such as `ofInt` are not declared. (d) The remaining members — `Queue.dequeueFirst` / `dequeueAll` / `Stack.popAll` / `popWhile` / `ArrayDeque.removeHead` and so on — rely on pickle supply, and whatever is not supplied becomes a diagnostic

- **The private runtime's `Tuple2` has no `toString`** (noticed in `agent/hkinfer`; a separate matter
  from auto-tupling. The same on main). Under `--no-scala-library`,
  `println((1, "a"))` prints `scala.Tuple2@…` instead of `(1,a)`. The same naturally applies to
  `println(1, "a")` with the parentheses omitted, so `hk_tuple_lib` is
  restricted to the jar. The right fix is to give the `TupleN` that `runtime.rs` generates
  a `toString` / `equals` / `hashCode`.

- **The `-Xlint:adapted-args` warning for auto-tupling** (`agent/hkinfer`, unimplemented).
  On auto-tupling nsc emits
  `adapted the argument list to the expected 2-tuple: add additional parens instead`
  (in 2.13.16 it is under `-Xlint:adapted-args`, not `-deprecation`).
  scala-rs has no framework for this lint, so it **accepts it without a warning**.

- **`case class` / `case object` inside a block** (a parser gap found in `agent/localtrait`,
  separate from local traits). A `case` at the head of a block statement is always read as a
  `case` clause, so

  ```scala
  def f(): Unit = {
    case class P(x: Int)   // error: expected pattern, found class
    println(P(1))
  }
  ```

  does not compile. nsc's shape is: in block-statement position, if the token after `case` is
  `class` / `object`, read it as a definition. **A diagnostic is emitted**, so nothing goes
  silently wrong. Ordinary (non-`case`) local `class` / `object` /
  `trait` do work.

- **No `needs to be abstract` for a class that does not implement its abstract members**
  (a pre-existing gap noticed in `agent/localtrait`. The same at top level, not just locally).

  ```scala
  trait L { def v: String; def plain = v + "?" }
  class LC extends L        // scalac: class LC needs to be abstract.
  ```

  Real scalac makes this an error, but we accept it, and it becomes an
  `AbstractMethodError` at run time. The `agent/localtrait` fix made the mixin
  forwarders come out correctly, but **this check itself is unimplemented**
  (`lt1_bad.scala` instead pins down `illegal inheritance; superclass … is not a subclass of …`,
  which is implemented as well).

- **Codegen for reading a member of the outer instance from a trait inside a class**
  (confirmed in `agent/localtrait`. **A pre-existing gap unrelated to whether it is local**;
  it belongs to the `$outer` work (`agent/nestedobj`), so it is not fixed in this slice).

  ```scala
  class Holder(val base: String) {
    trait Tag { def t = base + "!" }   // reads a member of the enclosing Holder
    class TC extends Tag
    def make(): String = new TC().t    // NoSuchFieldError: $outer
  }
  ```

  `Holder$Tag$class.t` emits a `getfield $outer` on `$this` (an interface type), so it becomes a
  `NoSuchFieldError` at run time. nsc does it the same way it handles captures: it puts an
  abstract accessor (`Holder$Tag$$$outer()`) on the interface and has the implementing class implement it
  from its own `$outer` field. The `trait_capture_accessors` introduced in `agent/localtrait`
  is exactly that shape, so laying the same mechanism over `$outer` is the right fix.
  A local trait shows the same symptom
  (before this slice the same code failed with `AbstractMethodError`, so this is
  not a regression).

- **The `inner_name` in `InnerClasses` for a local class** (`agent/localtrait`).
  The binary name is now `Main$LocalC$1`, the same as nsc, but for `InnerClasses`
  nsc's `inner_name` is the indexed `LocalC$1` while ours is the original `LocalC`.
  As a result **only `getSimpleName()` on a local class differs from nsc**
  (`EnclosingMethod` / `isLocalClass` / `isMemberClass` of `Main$LocalC$1`
  do match). It is written up in `inner_local_class_has_no_outer` in
  `crates/cli/tests/innerclasses.rs`.

- **Local declarations are visible from outside their scope** (confirmed in `agent/localtrait`.
  A pre-existing gap, the same on main).

  ```scala
  object Main { def mk(): Unit = { trait Local { def l = "l" } } }
  class TopUser extends Main.Local   // scalac: type Local is not a member of object Main
  ```

  Real scalac rejects this, but we accept it and emit a `TopUser` with no parent.
  The owner of a local declaration's symbol is the method, and yet name resolution of `Main.Local`
  reaches that far. **The reverse direction** (a top-level class implementing
  a local trait) is a shape that cannot be written in Scala in the first place, so
  the `agent/localtrait` fixtures do not cover it either.

- ~~**StackMapTable for `try` in argument position and for `while`**~~ →
  fixed in `agent/loopframe`. They were **two separate items, not the same root cause**
  (the "the frame at the top of a loop and a `try` on top of the operand stack" section).


- **`T$class` static helpers** (`agent/fewerclasses`, root 2 of "we emit more
  classfiles than nsc"; not attempted). nsc 2.13 compiles a trait's concrete
  methods to **default methods on the interface** and emits no `T$class` at
  all; `$init$` becomes a `static` method on the interface itself. scala-rs
  emits one `T$class` per trait with a concrete method — **106 of them** for
  slick, which after the closure-duplication fixes is the *entire* remaining
  gap to nsc (1552 against 1498; the closure count is 141 against nsc's 137).
  Moving it touches `emit_trait_impl_class` / `emit_trait_impl_method` /
  `emit_trait_init` in `crates/backend/src/gen.rs`, every `invokestatic
  <Iface>$class.m` call site (five in gen.rs), the mixin forwarders each
  implementing class emits, and separate compilation against classfiles that
  still have the old shape. It is a bigger change than the whole of
  `agent/fewerclasses` was.

- **`catch { case _: MatchError => … }` names the bare class in
  `--no-scala-library`** (`agent/fewerclasses`, found in passing; not fixed,
  and **not** specific to `PartialFunction`).

  ```scala
  object Main { def main(a: Array[String]): Unit = {
    val x: Any = 2.0
    try x match { case i: Int => println(i) }
    catch { case _: MatchError => println("caught") } } }
  ```

  Both real scalac and scala-rs's jar mode print `caught`; the private runtime
  gives `NoClassDefFoundError: MatchError` — the exception table holds
  `MatchError`, not `scala/MatchError`, although `throw`ing one from a `match`
  fall-through resolves correctly. So it is the *catch* type's resolution, not
  the class: `runtime.rs` does emit `scala/MatchError`.
