## Not implemented

The following are not implemented. They are not stubbed out to "pretend they work" either. The remaining language-side gaps and the remaining library-side gaps are listed separately.

Language:

- **The rest of def macro expansion**. Expansion itself works (see "def macro expansion
  (JVM bridge)" above). What is still missing:
  **whitebox macros** / **macro bundles** (`class B(val c: Context)`) /
  **pickling of macro bindings** (the `MACRO` flag and `@macroImpl`, which is why a
  macro def cannot be expanded from a *different run* — only the shape "macro def in
  the current run, implementation from a previous run" works) / **tags for inferred
  type arguments** (only an explicit `f[T]` is supported) / **`c.enclosingPosition` /
  `c.typecheck` / `c.inferImplicitValue`** (calling one makes the engine throw
  `UnsupportedOperationException`, and its name appears in the diagnostic) /
  **passing blocks, function literals, `new` and similar arguments (and the receiver)
  through to the implementation** / **`c.prefix` for a call written without a
  receiver** (nsc's `This(<enclosing class>)`) /
  **taking a class compiled in the same run as a type argument** (tags are built with
  `staticClass(<full name>)`, so the engine's mirror can only resolve classes on the
  macro classpath — that is, classes written by a *previous run*) /
  **tags for type parameters that have no tag** (nsc creates a free type symbol;
  scala-rs refuses). None of these silently expand to a different tree: each is
  reported with a reason, as
  `macro expansion is not implemented: cannot expand f (implementation Impl$.m):
  <reason>`
  (**[`docs/macros.md`](macros.md)** §7.11 / §7.12 / §7.13)
- **The rest of quasiquote expansion (reification)**. `q"..."` / `tq"..."` /
  `pq"..."` / `cq"..."` are lowered to `internal.reificationSupport.Syntactic*`
  calls and executed. Type ascriptions, eta expansion, blocks and `val`, `new`,
  `match`, partial functions, function literals, and type, pattern and `case`
  clauses all **match real scalac 2.13.16 under `showRaw`**
  (`tests/fixtures/qr_forms.scala`). Definitions (`class` / `case class` / `trait` /
  `object` / `def`, and `val`/`var` with modifiers) match as well
  (`tests/fixtures/dq_defs.scala`, 93 lines). Holes that are not `Tree`s are lifted
  to the trees the standard `Liftable` instances would produce
  (`tests/fixtures/lf2_lift.scala`). The three forms that need fresh names — the `_`
  placeholder, `_` as a type argument (an existential), and right-associative
  operators such as `a :: b` — are built with the same per-block `freshTermName` /
  `freshTypeName` blocks as nsc (`tests/fixtures/fn2_fresh.scala`).

  What remains: forms the parser normalises away along with the distinction nsc
  preserves (`if` without `else`, by-name types, by-name and vararg parameters,
  procedure syntax, pattern definitions, self types, early definitions); mixing
  `..$` with ordinary arguments; `type` definitions; `class` / `def` definitions;
  holes whose type has no standard instance (`liftList`, `liftTuple*`, and so on);
  collection operations in the reflect API (`MemberScope#collect`); `TypeTag`
  materialization; and `reify { … }`. `TypeTag` / `WeakTypeTag` materialization is
  **implemented for monomorphic types**; parameterised types and nested classes are
  refused by name (§7.10). Every one of these is reported **by name** — the specific
  form is called out — as `unimplemented syntax: quasiquote ... (which form)`,
  `a hole of type X is not lifted (…)`, or `cannot expand reify { ... }`; none of
  them is silently accepted. What each would require is listed in
  [`docs/macros.md`](macros.md) §7.7 / §7.8 / §7.10.

  For slick's `ShapedValue.mapToImpl`, putting scala-reflect.jar on `-cp` cut the
  errors from 20 down to single digits. The source records this reduction twice, in
  two different revisions: once as 20 → 7 (remaining causes: `Liftable`,
  `symbolOf[R]`, and the three fresh-name forms) and once as 20 → 9 (remaining
  causes: `Liftable`, `symbolOf[R]`, and `TypeTag` materialization).
- A full nsc pickle. What is emitted is a subset: TERMname / TYPEname / TYPEsym / CLASSsym / MODULESYM / VALsym / EXTref / EXTMODCLASSref / METHODtpe / POLYtpe / TYPEREFtpe / CLASSINFOtpe / TYPEBOUNDStpe / THIStpe / SINGLEtpe / NOPREFIXtpe / CONSTANTtpe / LITERALint / LITERALboolean / LITERALstring and the other literals / EXISTENTIALtpe / REFINEDtpe / SYMANNOT / ANNOTATEDtpe / ANNOTINFO / TREE (IDENTtree / SELECTtree / THIStree / SUPERtree / APPLYtree). ByteCodecs is SID-10. The wire format is the same as nsc's: nentries plus big-endian Nat. `val`s become METHOD|STABLE|ACCESSOR getters plus a NullaryMethodType. Case classes get CASE plus CASEACCESSOR on the fields. Flags are the nsc raw long passed through `rawToPickledFlags` (VARARGS / BRIDGE / JAVA are emitted where they apply). The coverage is whatever scalac 2.13.16 needs to typecheck `val` / `def` with parameters / `id[T]` / `new Point` plus `p.x` / the companion apply `Point(...)` / the term `Point` / the extractor `unapply` / a `def` in an object / `def f(xs: List[_]): Int` / `@deprecated("msg", "2.13.0") def g` / `def me: this.type` / `def f(xs: List[_ <: AnyRef])` / `def h(x: Int @unchecked)` / `val one: 1` / `def lit(x: 1)` / `def nest(xs: List[_ <: List[_]])` / `def idRef(x: MixA with MixB { def f: Int })` / `@Ann(foo)` / `@Ann(c.x)` / `@Ann(this)` / `@Ann(classOf[Int])` / `@Ann(ident(1))` / `@Ann(this.x)` / `@Ann(super.foo)` / `@Ann(ident(ident(1)))` / `@Ann(foo = 1)` / `@Ann(foo = this.x)` / `@Ann(foo = bar)` / `Lib.join("a","b")` / `new OrdBox(1).compare(...)`. **Parameter clauses collapse into one** (pickling happens after `uncurry` has flattened `paramss` on the symbol, so `def bind(fa)(f)` reads back as `bind(fa, f)`). **The only parent in CLASSINFOtpe is `Object`** (the inheritance in `trait Monadic[F[_]] extends Functor[F]` does not make it into the pickle; it is present in the classfile's interfaces, so it does work when read back through `-cp` on this side). This is not a full pickle; the remaining gaps are the ones listed under Remaining.

Out of scope (diagnosed, or not parsed at all):

- Compiler plugins
- Scala 3 syntax and TASTy. Unknown entity references in XML literals are diagnosed (elem / text / splice / non-prefixed attributes / `xmlns:p` / prefixed attributes / prefixed element names / comments / CDATA / PI / `&amp;` `&lt;` `&gt;` `&quot;` `&apos;` / `&#N;` are implemented)
- Other forms of `forSome { val x: T }` (`p.Inner forSome { val p: Outer }` is implemented). The common unbounded `List[_]` / `T forSome { type X }`, the bounded `List[_ <: AnyRef]` / `List[X] forSome { type X <: AnyRef }`, and the nested `List[_ <: List[_]]` are implemented
- View bounds on higher-kinded type parameters: scalac 2.13.16 rejects every spelling — `F[_] <% Ordered[_]`, `F[_] <% Ordered[F[A]]` and the rest — with `type F takes type parameters`, and scala-rs reports the same diagnostic (a proper `T <% V` on a method or class is implemented). **Context bounds `F[_]: C` are a separate case**: scalac 2.13.16 accepts them (confirmed by measurement), so they are implemented — see "Type members and higher-kinded context bounds" below

Moved out of "out of scope" (implemented in this slice):

- **`implicit class` in a package object** (from another compilation unit in the same package, or via `import pkg._`; IMPLICIT in the pickle). The nested classfile `package$Rich` is exposed on `-cp` as the member `Rich` of the outer class. A top-level `implicit class` gets nsc's `` `implicit` modifier cannot be used for top-level objects ``. Without an import the enrichment is not visible. Synthesis of local implicit classes is untouched
- **Structural assignment** `x.foo = v` (for `{ var foo: T }`, or a getter plus `foo_=`) and structural `x(i) = v` (`update`). Both go through reflective `foo_=` / `update`, as in nsc 2.13. The illegal `{ def foo: Int }; x.foo = 1` gives `foo_= is not a member`
- scala-library 2.13.16's **`IndexedSeq` and `immutable.Queue`** (the real jar: `IndexedSeq(1,2)(1)` plus `enqueue` / `dequeue`). Missing members are diagnosed; no fake classfiles are emitted

Library:

- The complete Scala standard library. Without `--scala-library`, Option / List / FunctionN / Tuple2 come from the private runtime. **Even when linked against the jar**, the complete StringOps and the full set of numeric enrichments (`RichByte` and friends) are unsupported
- Implicit enrichment of the `scala.Int` companion (some of it, via the jar's `intWrapper`, is linked; the companion constants themselves, such as `Int.MaxValue`, are implemented in this slice — the extra methods on `RichInt` are a separate matter)
- The nested objects `Range.Int` / `Range.Long` / `Range.BigInt` / `Range.BigDecimal` (`Range$Long$` and the rest, which carry the `apply` / `inclusive` that return a `NumericRange`). `javap` confirms that `Range$` itself only has the `Int` versions
- `implicitly` when the expected type is a function type (`implicitly[Int => Ordered[Int]]`). `adapt_implicit_apply` returns early for eta expansion when the expected type is a `Type::Function`, so the implicit clause is never filled and the result stays a method type. Implicit **parameters** of function type (`def f(implicit ev: A => B)` and view bounds) are implemented; this is a separate gap on the `implicitly` side
- `List[Option[A]].flatten` (`List(Some(1), None, Some(3)).flatten`). The witness `scala.Option.option2Iterable[A](xo: Option[A]): Iterable[A]` is now supplied from the pickle and **does work as a view** (`List(1) ++ anOption` and `val xs: Iterable[String] = anOption` started compiling in `agent/ovl3`). What remains is on the `flatten[B](implicit asIterable: A => IterableOnce[B])` side: when a `Function1` is demanded as an implicit **value**, the implicit conversion *method* is not eta-expanded. **Today this is a diagnostic, not a silent miscompile** (`value mkString is not a member of ((Option[Int]) => IterableOnce[B])List[B]`)
- `Array[Array[A]].flatten` (`value flatten is not a member of Array[Array[Int]]`). `ArrayOps.flatten[B](implicit asIterable: A => IterableOnce[B], m: ClassTag[B]): Array[B]` is missing from the prelude
- An implicit clause with an undetermined type parameter in a direct argument position (`println(xs.flatten)`). `instantiate_undet_arg` pins the undetermined variable to its lower bound (`Nothing`) before the search runs, so the diagnostic names `IterableOnce[Nothing]`. Writing `val v = xs.flatten` resolves correctly

- Passing a pattern-match literal directly to `collect` on a sorted `Map`
  (`treeMap.collect { case (k, v) if … => (k, v) }`). `K2` stays `Any`, so the
  search goes looking for an `Ordering[Any]`. The single-type-parameter
  `TreeSet.collect { … }`, a `PartialFunction` value with a type ascription, and
  explicit type arguments `collect[K2, V2] { … }` all work (`agent/mismatch9`)
- `mutable.ArrayBuilder[T]` has no `Builder[T, Array[T]]` base type
  (`ArrayBuilder.make[E]` cannot be passed where a `mutable.Builder[E, Array[E]]`
  is expected)
- `Equiv[Int]` (`agent/ordsummon`). The summon itself now resolves to
  `Equiv.apply[T]`, but the prelude does not model the real ABI's
  `Ordering[T] extends PartialOrdering[T] extends Equiv[T]`, so the result is
  `could not find implicit value of type Equiv[Int]` (**a diagnostic, not a
  miscompile**; real scalac passes `Ordering.Int`). This is a matter of adding one
  edge of the same shape as `Numeric[T] <: Ordering[T]`, but it changes the implicit
  scope of `Ordering`, so it is treated as a separate slice
- `Ordering#compare` is still typed `(Any, Any): Int` in the prelude. Real scalac
  rejects `Ordering[String].compare(1, 2)`; scala-rs accepts it
  (`agent/ordsummon`'s `os2_summon_bad.scala` deliberately omits this line)

The parser does not silently discard unsupported syntax: it emits a diagnostic and an `Unimplemented` node.
