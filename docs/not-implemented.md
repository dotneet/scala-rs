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
- **Default constructor arguments of a class with *no companion*.** A class that has one — every `case class`, and any class the source gives an `object` — now carries nsc's `$lessinit$greater$default$N` (and `apply$default$N`) on the companion module class, so a separately compiled caller links: real scalac accepts `Top(1)` against our `case class Top(length: Int, varying: Boolean = true)` and calls `Top$.apply$default$2()`. See `crates/typer/src/ctor_defaults.rs`. What is left is the case nsc handles by *synthesizing* a companion for `class Box(val a: Int, val b: Int = 7)`; doing that here would add classfiles, so `new Box(1)` from another run still says `not enough arguments for constructor Box`. This is 12 of the 17 remaining classfiles scalac writes for slick and we do not; see [docs/notes/companions-and-class-symbols.md](notes/companions-and-class-symbols.md) `agent/ctorgaps2` synthesizes one for a **narrower** shape that cannot do without it: a default in a *later parameter clause*, whose getter takes the earlier clause's parameters and so cannot be spliced at the call site (`Typer::needs_ctor_default_companion` / `emit_ctor_default_companion`). A first-clause default may legally name nothing, so it is still spliced and still adds no classfile — which is why slick's count is unchanged at 1490.
- **Writing `private[p]` / `protected[p]` into our own pickle.** The *reading* half is closed (`agent/ctorgaps2`): `Member::private_within` now carries the boundary's simple name, `PickleSupply::install_ctor` copies it onto the constructor, and `Typer::access_within_of` walks out from the member's owner to find it, so a `private[libp]` constructor in a class file **real scalac wrote** is refused from another package with nsc's own sentence and still accepted from inside `libp`. What is left is the writer: `backend::pickle::pickled_access_flags` drops the flag for a qualified-private member and emits no `privateWithin` entry at all, so the boundary is absent from a class file *we* wrote — our reader and real scalac both accept `new libp.Qual("x")` against it. Closing that is a pickle-format change (`SymInfo` gains a symbol reference, moving every entry after it), not a flag. Note for whoever takes it: nsc emits **no `PRIVATE` flag** for `private[p]` — scalac 2.13.16's pickle for `class Qual private[libp] (...)` gives its `<init>` `flags=0x200` with `PRIVATE` and `PROTECTED` both clear — so the earlier note that it is "the bare `PRIVATE` flag plus a reference" was wrong. Pinned by `crates/cli/tests/ctorgaps.rs`'s `our_own_class_file_does_not_yet_carry_the_qualified_boundary`
- **Solving a class's type parameter from an omitted constructor default.** `case class C[+F <: Option[Int]](n: String, f: F = None)` called as `C("q")`: nsc reads the getter's inferred result type (`None.type`) and takes `F` from it. The typer splices the stored expression instead and checks it against the still-unsolved `F`, so this reports `type mismatch; found: None$ required: F`. The getter itself is emitted with nsc's descriptor (`()Lscala/None$;`) — only the call site does not consult it. slick compiles because its own such call sites pass the argument or fix `Fetch` elsewhere

- **Declaring a default whose expression does not conform to the parameter's own type parameter.** `def f[T](x: T = ())` — nsc infers the default *getter*'s result type (`f$default$1[T]: Unit`) rather than checking the expression against `T`, and accepts it; scala-rs checks against `T` and reports a mismatch. scalatra's `halt[T: Manifest](status = null, body: T = (), …)` is this shape. **Reading** such a method back out of a class file works, which is what the libraries need — see [docs/default-arguments.md](default-arguments.md) and `tests/multi/defaultargs_binary/Halt_1.scala`
- **Curried defaults on a `-cp` class that scala-rs itself compiled.** The typer reads pickles from the standard library and from classes `adopt_binary_class` takes over, not from arbitrary `-cp` output, so such a class is described by its class file alone — and a class file flattens the parameter clauses. `def join(a: String)(b: String = "-")(c: String = a + b)` comes back as one three-parameter method and `join("a")()()` does not typecheck. Against **nsc's** class files the pickle is read and the same declaration works

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

Compiler flags (`agent/xflags`):

- **`-Xasync`: the async state machine.** The flag is accepted and reaches
  macros through `c.compilerSettings` (which is where scala-async's
  "The async requires the compiler option -Xasync" comes from), but
  `scala.tools.nsc.transform.async` — the transform that rewrites an `async {
  ... await(f) ... }` block into a `FutureStateMachine` subclass — is not
  implemented, and neither is `c.internal.markForAsyncTransform`, the hook the
  library calls to ask for it.
- **`scala.async.Async.async` cannot even be named.** A macro *definition* is
  carried only in a class file's `ScalaSignature` pickle, and this compiler
  recognises macro defs from source only (`crates/typer/src/macros.rs`). So
  `import scala.async.Async.async` is reported as `value async is not a member
  of object scala.async.Async`, where scalac reports the library's own
  `-Xasync` message.
- **`-Xsource-features`: nine ignored features and one partial feature.**
  `case-apply-copy-access` is implemented; `infer-override` is partial (below).
  The remaining features
  (`case-companion-function`, `case-copy-by-name`,
  `any2stringadd`, `unicode-escapes-raw`, `string-context-scope`,
  `leading-infix`, `package-prefix-implicits`, `implicit-resolution`,
  `double-definitions`) are parsed and validated, and warn when named one by
  one, but change nothing. Naming a group (`_`, `v2.13.14`) does not warn,
  because `-Xsource:3-cross` expands to `_`.
- **`-Xsource:3` migration warnings.** nsc reports, as errors under
  `-Xsource:3`, where a Scala 3 behaviour would differ — including "access
  modifiers for `copy` / `apply` method are copied from the case class
  constructor under Scala 3". We do not have the `scala3-migration` warning
  category, so none of these is reported.
- **`copy$default$N` access.** With `-Xsource-features:case-apply-copy-access`
  scalac makes these getters as private as `copy` itself. We leave them public:
  nothing in Scala source can name them, and this compiler fills an omitted
  `copy` argument at the call site rather than through the getter.
- **The specialization phase.** `@specialized` / `@unspecialized` are accepted
  and what they select is recorded on the symbol, but no `Foo$mcI$sp` class and
  no `f$mcI$sp` method is emitted, so classes we compile are not ABI-compatible
  with what real scalac produces for the same source. `tests/spec_classfiles.sh`
  measures the gap: over the corpus's 37 `pos/spec-*` tests scalac emits 700
  specialized classes and we emit none. See
  [docs/specialization.md](specialization.md).
- **The value class *implementation restrictions*.** The eight rules
  `neg/valueclasses.check` records — a `trait` may not extend `AnyVal`, a value
  class may not be nested or local, must have exactly one `val` parameter that
  is neither `private[this]` nor `protected[this]`, may not take a `var`, may
  not declare a field, and may not have a `@specialized` type parameter — are
  now checked (`crates/typer/src/valueclass.rs`), and so is most of nsc's
  `checkEphemeral` beside them: a nested class, trait or object (at any depth
  inside a `def` body) and a body statement that is not a definition, under
  "implementation restriction: … is not allowed in value class" and its
  second line. The same function's *other* caller is also implemented — a
  **universal trait** (`trait T extends Any`, SLS 5.3.3) may declare only
  `def`s, types and imports. `neg/valueclasses-impl-restrictions` and
  `neg/anytrait` both match their `.check` files exactly.
  What is still *not* checked is `checkEphemeral`'s three `DefDef` arms — a
  secondary constructor, a redefined `equals` / `hashCode`, an "additional
  parameter" — and the "qualified super reference" arm of its deep traversal.
  All four read nsc-specific symbol flags (`isAuxiliaryConstructor`,
  `isSynthetic`, `isParamAccessor`) that this tree does not carry in the same
  sense, none appears in either `.check` file, and each is a *new rejection*:
  left out rather than guessed at.
- **`agent/nameamb` closed the definition-vs-deeper-import ambiguity; three
  narrower shapes of the same rule are still owed.** SLS 2 gives a definition
  higher precedence than an import, but nsc only lets precedence decide when
  the two sit at the same nesting level; a **deeper** import is consulted
  (`Contexts.lookupSymbol`, while `imp1.depth > symbolDepth`) and then
  `defSym` and `impSym` together are `ambiguousDefnAndImport`.
  `SymbolTable::ambiguous_defn_and_deeper_import` now reports it, in nsc's
  three lines, and `neg/name-lookup-stable`, `neg/ambiguous-same`,
  `neg/t8024b` and `neg/specification-scopes` all match their `.check` files
  at the lines scalac reports. What is left:

  1. **A definition made by this unit's own package clause.** `package p;
     object X; class G { def pick = { import Imp._; X } }` is
     "reference to X is ambiguous; it is both defined in package p …" to
     scalac, and we take the definition silently. The binding is not in the
     package-clause scope when the reference is typed:
     `Typer::expose_same_unit_package_def` enters it into whatever scope
     happens to be current, which is the import's own, so the two come out at
     the same nesting level. Entering it at the depth the clause actually
     binds it is what closing this needs, and that scope is shared for the
     whole unit — the reason it is injected lazily today.
  2. **Two *imports* at different nesting levels.** An outer explicit import
     and an inner wildcard one are `ambiguousImports` to nsc ("it is imported
     twice in the same scope by / … / and …"), by
     `Contexts.resolveAmbiguousImport`'s `!imp1Explicit && imp2Explicit` when
     the depths differ. `Scope` now has everything this needs;
     `neg/specification-scopes` line 21 is the case, and we report its line 15
     and not its 21.
  3. **The *type* namespace.** The rule is implemented for term references
     (`Typer::type_ident` and the stable-id pattern) only. Nothing in the
     corpus's `neg` set turns on the type half, and the type lookup has a
     module fallback that the term one does not, so it was left rather than
     guessed at.

  A fourth difference is cosmetic and recorded so nobody re-derives it: nsc
  spells the owner of a definition made inside a `locally { … }` block
  "value <local Y>", and we say "object Y" — the enclosing anonymous value is
  not a symbol here. `neg/specification-scopes` is the only place it shows.
- **An *enclosing* template's self type, for a bare name written in a nested
  one.** `trait Q { self: PriorityQueue[Int] => trait Inner { def d = dequeue() } }`
  is `not found: value dequeue`; the same call written directly in `Q`'s body
  resolves (root 24 in [docs/gitbucket.md](gitbucket.md)). nsc's context chain
  reaches the outer self type, but the member has to be read at the *outer*
  `this` — `dequeue(): Int`, called on `Q.this`, not the declared `A` on
  whatever `this` happens to be — and entering the symbol alone gives neither.
  Reported, not answered wrongly.
- **Remaining tail-call shapes.** Direct self tail calls in ordinary methods
  become loops, including receiver changes, curried arguments, lifted local
  definitions, value-class `$extension` statics, and the right operand of
  `Boolean.&&` / `Boolean.||`. Explicit returns and try/catch/finally tail
  positions remain conservatively rejected by the typer. Mutual recursion is
  not transformed. See [tailrec.md](tailrec.md) for the precise scope and
  differential execution tests.
- **`Stream`'s `tail` does not collapse to one member.** The five remaining
  `@tailrec` diagnostics on the scala library — `Stream.foreach`, `find`,
  `foldLeft`, `collect`, `collectFirst` — are *cascades*, not tail-position
  defects. Each is preceded by `value foreach is not a member of <overload
  Iterable[A] | Stream[A] | Stream[A]>`: `Stream.tail` arrives as an
  unresolved three-way overload, so `tail.foreach(f)` binds no symbol at all
  and the tail-call scan then honestly reports "contains no recursive calls".
  Reduce `tail` to one member and all five go away together; nothing in
  `check_tailrec` needs touching. Twenty-three library errors carry that
  member-of-an-overload message, so the cluster is larger than the five.
- **`super` in a class whose `with` list is not an antichain.** `class C1
  extends L3 with L1`, where `trait L3 extends L1 with L2`, linearizes to
  `C1 L3 L2 L1 L0` — the typer has this right, and it is scalac's answer — but
  the program prints `C1 L1 L0`. `Typer::super_select_member`
  (`crates/typer/src/check_pattern.rs`) walks the *written* parent list
  reversed, on the reasoning that a later mixin is the more specific one; that
  holds only while the list is an antichain, and `L1` is already an ancestor of
  `L3`. So `super.t` resolves to `L1.t`, two traits drop out of the chain, and
  nothing is reported. The emitted `$$super$` accessors are byte-identical to
  scalac's; the single wrong instruction is `C1.t()`'s `invokestatic L1.t$`.

  Walking `lin::linearize` instead is the rule SLS 6.5 states and does fix this
  shape, but it cannot land on its own: it costs five new errors on the scala
  library (`1552 -> 1557`), all one family, and they pin a second defect
  underneath. `super_select_member` returns a single class that serves both as
  the search entry *and* as the type prefix, and the reversed-parent walk was
  compensating for the prefix being wrong. `final class LazyList[+A] extends
  AbstractSeq[A] with LinearSeq[A] with LinearSeqOps[A, LazyList, LazyList[A]]`
  has exactly the redundant-mixin shape (`LinearSeq` already extends
  `LinearSeqOps`), so the reversed walk stopped on the `…Ops[A, CC, C]` mixin
  that carries the bindings and read `super.diff` as `LazyList[A]`; the
  linearization stops on `LinearSeq` first and reads it as `Seq[A]`. nsc has no
  such dependence, because it types `super.m` as `m.tpe.asSeenFrom(this.type,
  m.owner)`.

  Making the prefix `this`'s own type is closer to nsc and fixes most of them
  (`1552 -> 1556`, `files_with_errors 168 -> 167`), but four survive, and they
  are a *third* defect. **That third one is now fixed** and this entry is what
  is left. `SymbolTable::subst_as_seen_from` used to reach a base class through
  whichever parent its walk arrived at first, so a class reachable at two
  instantiations was read at the first in written-parent order rather than the
  most derived; `SymbolTable::base_type_args` now decides that once, per SLS
  5.1.2's linearization, and the walk uses its answer whatever path it took
  (`1551 -> 1420` errors on the scala library, `168 -> 166` files, no file
  worse; cats `215 -> 211`; slick byte-identical but for one redundant
  `checkcast`, see below). `crates/cli/tests/bts.rs` dual-runs both shapes
  against scalac 2.13.16. So the `super` walk is now the next step, and it
  should be re-measured rather than assumed: the five errors it cost were
  attributed to the prefix and to this, and only one of the two has moved.

  Measured on `agent/linorder2` at `66732045`; `crates/typer/src/lin.rs`'s own
  `+:` fix is independent of all three and is byte-identical everywhere.
- **An override declared by a mixin narrower than the one the member is
  inherited from.** `TreeMap[Int, String].updatedWith` is
  `type mismatch; found: Map[Int, String]`, where scalac says
  `TreeMap[Int, String]`. This is *not* the base-type instantiation above --
  `immutable.SortedMapOps` genuinely extends `MapOps[K, V, Map, C]`, so
  `MapOps`' own `updatedWith` really does return `Map[K, V1]` there -- it is
  that `immutable.SortedMapOps` **overrides** `updatedWith` with the sorted
  `CC[K, V1]` (`SortedMap.scala:108`) and that declaration is not the one
  selected. `VectorMap`, which has the same hierarchy shape but no such
  override, is correct and is pinned in `crates/cli/tests/bts.rs`. Found while
  fixing the entry above; not attempted in the same slice.
- **A partial function whose parameter is a written tuple type emits no entry
  cast.** `gen_lambda::pf_bind_arg_and_captures` casts the incoming `Object`
  when the parameter is a `Type::Class`, and `emit_unbox` has no case for
  `Type::Tuple`, so `{ case (p, (_, Some(s))) => … }` over a `Map` gets no
  `checkcast scala/Tuple2` where the same type spelled `Tuple2[A, B]` does.
  Nothing observable follows -- the pattern's own `instanceof scala/Tuple2` is
  emitted either way, `applyOrElse` never had the cast, and slick verifies and
  runs -- but the two spellings of one type should not emit different code.
  `checkcast_internal` already has the tuple case; adding it to that branch
  makes slick's `RewriteJoins$$anonfun$42` byte-identical again and changes two
  other lambdas instead, which is why it was left out of a typer slice.
- **Every cycle in a tangle of overlapping `extends` cycles, and nsc's second
  cyclic diagnostic.** A cyclic inheritance graph is now rejected with
  `illegal cyclic reference involving trait X`, at scalac's line and with
  scalac's wording, but only for the first cycle each template's parent walk
  reaches. For `trait A extends B with C; trait B extends C with D;
  trait C extends D with A; trait D extends A with B` plus a second, disjoint
  cycle, scalac 2.13.16 prints seven errors — one `illegal cyclic reference`
  per cycle plus five `illegal cyclic inheritance involving trait …` from
  `validateParentClasses`, which this compiler has no equivalent of — and we
  print the first of them. The program is rejected either way; the count and
  the follow-on diagnostics are not reproduced.
- **A downstream type mismatch suppressed by a repaired cycle.** Once
  `illegal cyclic reference` is reported, the closing parent edge becomes
  `Type::Error`, and `is_sub_type`'s `(Type::Error, _) => true` arm then accepts
  everything that class is passed to. For `trait X extends Y with Z; trait Y
  extends Z with W; trait Z extends X with W; trait W extends X with Y` plus
  `val q: Q = (x: X)`, scalac 2.13.16 prints three errors -- two cyclic and one
  `type mismatch; found: X, required: Q` -- and this compiler prints the two
  cyclic ones. Absorbing follow-on errors is the point of an error type, so this
  is a fidelity gap rather than a wrong acceptance: the program is rejected
  either way. Noted while guarding `SymbolTable::is_sub_type`, whose own
  termination no longer depends on that repair.
- **`Iterator.GroupedIterator` is reachable as `Iterator.GroupedIterator`, and
  scalac says it is not.** `object Iterator` has no such member — the class is
  declared inside `trait Iterator` — but a nested class file's JVM name does
  not say whether the outer name is the class or the object, so
  `java_class_owner` answers the class and `enter_in_companion_scope` then puts
  the same symbol in the object's scope. `def f(g: Iterator.GroupedIterator[Int])
  = g` compiles here; scalac 2.13.16 reports `type GroupedIterator is not a
  member of object Iterator`. Found while fixing `agent/basetypeargs`; not
  attempted, because the rule that puts it there is the one that makes
  `Resource.ExitCase` work and narrowing it needs the pickle's own owner rather
  than the JVM name.
- **`IterableOps.grouped(size): Iterator[C]` reads `C` as a base of the receiver
  in a large run.** cats' `NonEmptyLazyList.scala:462` gets `Iterable[A]`,
  `NonEmptyVector.scala:357` gets `Seq[A]`, and `NonEmptySeq.scala:364` and
  `instances/stream.scala:64` are the same. **It does not reproduce on the file
  alone** — `NonEmptyVector.scala` compiled by itself with the measure's flags
  and classpath does not contain the error, and a fifteen-line reproduction of
  the same value class compiles — so the receiver's shape depends on what else
  is in the run and this is a supply-order question, not an expression one.
  `docs/cats.md`'s `agent/basetypeargs` section has the measurement.
- **A nested class named in a pickled signature outside `scala.collection` is
  still entered as a package-level class with a `$` in its simple name.**
  `PickleSupply::ensure_class` now splits the JVM name the way
  `java_class_owner` does -- but only for a class nested in `scala.collection`
  whose pickled parents reach `IterableOnce`, because that is the family where
  the defect was measured and the only one where lifting it is free. Elsewhere the
  twin stands: `scala/reflect/api/Exprs$Expr` is entered beside the `Exprs.Expr`
  that `prelude_reflect` builds by hand, and unifying them costs
  `engine.rs::rd_reify_shape_expands_and_runs` (`value apply is not a member of
  Expr`, for `c.universe.Expr.apply[Int](...)`) because `reify*.rs` and
  `macros.rs` reason about the prelude's symbols. The general repair is to make
  the reflect surface come from the pickle like everything else, or to give
  those passes the symbol rather than the name; neither is a small change.
  Measured on `agent/basetypeargs`; `docs/cats.md` has the numbers.
- **A SAM literal is always an anonymous class; scalac uses `invokedynamic`
  where it can.** `crates/cli/tests/indy.rs` is the record of the split this
  compiler makes: a `FunctionN` literal becomes `invokedynamic` +
  `LambdaMetafactory`, everything else -- `PartialFunction`, and every SAM type
  -- becomes a closure classfile. scalac 2.13.16 also uses `invokedynamic` for
  a SAM type it considers a functional interface: for one file declaring
  `Equiv[Int]`, `Ordering[Int]`, `Hashing[String]` and `Runnable` literals it
  emits three `invokedynamic` call sites and *one* anonymous class
  (`S$$anonfun$o$2`, for `Ordering`), so the two compilers agree on one of the
  four. The difference is in class count and call-site shape, not in
  behaviour; `crates/cli/tests/samconv.rs` runs the same source under both and
  the output is identical. Measured on `agent/samconv`.
- **`SymbolTable::sam_sig` cannot see an override the pickle has not been asked
  for.** `PickleSupply` installs a library class's members one name at a time,
  so an override nothing has referenced is indistinguishable from an override
  that is not there, and a class whose sole abstract method is only sole
  *because* of such an override reads as having two. `scala.math.Ordering` is
  the case that mattered (`equiv`, inherited deferred from `Equiv`, overridden
  concretely) and `Checker::sam_sig_here` handles it by reading the class's
  concrete member names straight out of the pickle. That answers the question
  for one class at a time, on the SAM path only. The general form -- any
  question about a library class's member set is answered against a partial
  set -- is untouched, and *installing* the missing members instead is not the
  repair: doing so cost cats' `NonEmptyVector.scala` a diagnostic, because
  completing `coll` / `toIterable` / `fromSpecific` on `Vector` changed which
  `grouped` and `lazyZip` later expressions saw. Measured on `agent/samconv`.
- **`val v` beside a hand-written `def v_=` is reported as `reassignment to
  val`; scalac calls the setter.** `class C { val v = 1; def v_=(x: Int) = () }`
  followed by `c.v = 2` compiles under scalac 2.13.16 and is refused here.
  `setter_assign_lhs` asks the setter question only when the left side resolved
  to a *method* (a getter), and a `val` defined in the same run is a `Term`, so
  the rewrite is never offered. Widening the test to "a `_=` exists on the
  qualifier's class, whatever the left side resolved to" would also rewrite
  `this.v = 3` on a `var` of the enclosing class into an accessor call, which
  is a codegen change on the commonest assignment there is, so it was measured
  as out of proportion to the shape. Found by `agent/varassign`'s 44-case
  two-directional probe; it is the one case of the 44 that still disagrees.
- **Assignment to a wildcard-imported `var` uses `this` as the receiver.**
  `object O { var ov = 1 }; import O._; ov = 5` compiles and emits
  `aload_0; checkcast O$; putfield O$.ov`, which throws `ClassCastException` at
  run time (`class M$ cannot be cast to class O$`). The *read* of the same name
  is correct — `println(ov)` emits `getstatic O$.MODULE$` — so the defect is in
  the assignment path's prefix, not in import resolution. It is not in the
  mutability family `agent/varassign` closed (the symbol's mutability is read
  correctly; the receiver is wrong), and the qualified form `O.ov = 5` is
  correct, which is why no measure has ever shown it.

`-Xsource-features:infer-override` now has partial support under `-Xsource:3`:
ordinary inferred methods, vals and vars adopt the inherited type, while final
constant vals retain their narrower inferred type. Macro-related exceptions have
not been validated, so the CLI still reports partial support. It is not an ignored
flag, and it is not yet advertised as a fully implemented source feature.
