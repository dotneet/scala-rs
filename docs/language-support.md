## Implemented language subset

The syntax is Scala **2.13**. There is no Scala 3 `then`, no top-level definitions, no TASTy. The entry point is `def main(args: Array[String]): Unit`.

## Contents

- [Uncurry / Erasure](#uncurry-erasure)
- [Lambdas as `invokedynamic` (`agent/indy`)](#lambdas-as-invokedynamic-agentindy)
- [Method type-parameter inference (arguments plus expected type)](#method-type-parameter-inference-arguments-plus-expected-type)
  - [Undetermined type variables (nsc's undetermined type variables)](#undetermined-type-variables-nscs-undetermined-type-variables)
  - [The expected type is also a prototype for the arguments (nsc's `protoTypeArgs`)](#the-expected-type-is-also-a-prototype-for-the-arguments-nscs-prototypeargs)
  - [Empty varargs and `xs: _*`](#empty-varargs-and-xs-_)
  - [Dependent method types (nsc's `dependentTypeMap`)](#dependent-method-types-nscs-dependenttypemap)
  - [Higher-kinded application (`F[B]`)](#higher-kinded-application-fb)
- [Implicit resolution](#implicit-resolution)
  - [Polymorphic implicit def / implicit val](#polymorphic-implicit-def-implicit-val)
  - [Retyping a call whose implicit arguments were already filled in](#retyping-a-call-whose-implicit-arguments-were-already-filled-in)
  - [Residual implicit clauses in argument position](#residual-implicit-clauses-in-argument-position)
  - [Type parameters only the implicit search can determine](#type-parameters-only-the-implicit-search-can-determine)
  - [Filling a function-typed implicit parameter (a view) from an implicit def](#filling-a-function-typed-implicit-parameter-a-view-from-an-implicit-def)
  - [Local-scope implicit conversions (views)](#local-scope-implicit-conversions-views)
  - [Do not silently eta-expand an implicit clause that was never filled](#do-not-silently-eta-expand-an-implicit-clause-that-was-never-filled)
  - [Gaps in the prelude](#gaps-in-the-prelude)
- [Trait mixin](#trait-mixin)
  - [A trait inheriting a class (SLS 5.3.3)](#a-trait-inheriting-a-class-sls-533)
  - [Unresolvable parents are reported](#unresolvable-parents-are-reported)
- [The cake pattern across multiple compilation units (header pass)](#the-cake-pattern-across-multiple-compilation-units-header-pass)
- [implicit and default arguments of a parent constructor](#implicit-and-default-arguments-of-a-parent-constructor)
- [try / catch / finally](#try-catch-finally)
- [Unreachable code](#unreachable-code)
- [Nested types](#nested-types)
- [lazy val](#lazy-val)
- [Signatures of members without type annotations (lazy completer)](#signatures-of-members-without-type-annotations-lazy-completer)
- [Type aliases (alias type members)](#type-aliases-alias-type-members)
  - [Type aliases in a jar's package object](#type-aliases-in-a-jars-package-object)
- [super and qualified this](#super-and-qualified-this)
- [sealed and exhaustiveness](#sealed-and-exhaustiveness)
- [unapply / unapplySeq](#unapply-unapplyseq)
- [`x @ Pat` bindings and `null`](#x-pat-bindings-and-null)
- [Nested patterns (`case P(v) :: t`)](#nested-patterns-case-pv-t)
- [A `match` that falls through (`MatchError`)](#a-match-that-falls-through-matcherror)
- [AnyVal (value classes and universal traits)](#anyval-value-classes-and-universal-traits)
- [Boxed types (`java.lang.Integer` and `scala.Int`)](#boxed-types-javalanginteger-and-scalaint)
- [The numeric widening tower and `Byte` / `Short`](#the-numeric-widening-tower-and-byte-short)
- [Predef (this slice)](#predef-this-slice)
- [Import resolution](#import-resolution)
- [Singleton types `X.type` and namespaces](#singleton-types-xtype-and-namespaces)
- [The `Ordering` companion and summoning (`agent/ordsummon`)](#the-ordering-companion-and-summoning-agentordsummon)
- [Summoning `Equiv[T]` and `Ordering <: PartialOrdering <: Equiv` (`agent/eqtail`)](#summoning-equivt-and-ordering-partialordering-equiv-agenteqtail)
  - [The prelude type of `Ordering#compare` (same slice)](#the-prelude-type-of-orderingcompare-same-slice)
  - [Silently accepting `new T` / `new A` (a remaining item from `agent/parentcheck`, same slice)](#silently-accepting-new-t-new-a-a-remaining-item-from-agentparentcheck-same-slice)
- [When a newline ends a statement (nsc's `inLastOfStat` / `inFirstOfStat`)](#when-a-newline-ends-a-statement-nscs-inlastofstat-infirstofstat)
- [Function literals in block position (nsc's `expr(InBlock)`)](#function-literals-in-block-position-nscs-exprinblock)
- [`?` wildcard types and `-Xsource:3` `&` intersection types](#wildcard-types-and--xsource3-intersection-types)
- [Type members that take type parameters, and higher-kinded context bounds](#type-members-that-take-type-parameters-and-higher-kinded-context-bounds)
- [Getting slick's 7 generated files (`.fm` templates) to compile](#getting-slicks-7-generated-files-fm-templates-to-compile)
- [Constructor argument accessors and `FunctionN.tupled`](#constructor-argument-accessors-and-functionntupled)
- [Making case classes `Product`s (`agent/product`)](#making-case-classes-products-agentproduct)
- [Overload candidate sets (inheritance, `private[this]`, `java.lang.String`)](#overload-candidate-sets-inheritance-privatethis-javalangstring)
  - [`private[p]` resolves outward from the definition site](#privatep-resolves-outward-from-the-definition-site)
- [Type members, `this.type`, and cleaning up undetermined variables (`type mismatch`, slice 3)](#type-members-thistype-and-cleaning-up-undetermined-variables-type-mismatch-slice-3)
- [Premature alias completion and `FunctionN` (`type mismatch`, slice 4)](#premature-alias-completion-and-functionn-type-mismatch-slice-4)
- [Traits extending a function type, and omitted type arguments (`type mismatch`, slice 5)](#traits-extending-a-function-type-and-omitted-type-arguments-type-mismatch-slice-5)
- [Lubs, and three cases that pass typechecking and then fail (`type mismatch`, slice 6)](#lubs-and-three-cases-that-pass-typechecking-and-then-fail-type-mismatch-slice-6)
- [lub of captured parameters and invariant arguments (`type mismatch`, slice 7)](#lub-of-captured-parameters-and-invariant-arguments-type-mismatch-slice-7)
- [Expected types, varargs, dependent method types (`type mismatch`, slice 8)](#expected-types-varargs-dependent-method-types-type-mismatch-slice-8)
- [Higher-kinded expected types, overloads on sorted collections, `copy` inside a class (`type mismatch`, slice 9)](#higher-kinded-expected-types-overloads-on-sorted-collections-copy-inside-a-class-type-mismatch-slice-9)
- [The two class-header passes and `collect` on sorted maps (`type mismatch`, slice 10)](#the-two-class-header-passes-and-collect-on-sorted-maps-type-mismatch-slice-10)
- [Type-parameter capture in inherited members, and erased parents (`type mismatch`, slice 11)](#type-parameter-capture-in-inherited-members-and-erased-parents-type-mismatch-slice-11)
- [Type-constructor bounds, an `apply` of its own, implicits inherited by a companion (`type mismatch`, slice 12)](#type-constructor-bounds-an-apply-of-its-own-implicits-inherited-by-a-companion-type-mismatch-slice-12)
- [Views brought in by `import <value>._` (`agent/tail2`)](#views-brought-in-by-import-value_-agenttail2)
- [Higher-kinded implicit matching for `BuildFrom` (`LazyZip2`, `agent/buildfrom2`)](#higher-kinded-implicit-matching-for-buildfrom-lazyzip2-agentbuildfrom2)
- [Block values were double-boxed (erasure)](#block-values-were-double-boxed-erasure)
- [Reading jar classes from the pickle](#reading-jar-classes-from-the-pickle)
- [`super.m` is seen from `this.type`, not from the parent (`agent/lastone`)](#superm-is-seen-from-thistype-not-from-the-parent-agentlastone)
- [Operator-named `val`s were not encoded as field names](#operator-named-vals-were-not-encoded-as-field-names)
- [`slick_subset.sh` was discarding files because of warnings](#slick_subsetsh-was-discarding-files-because-of-warnings)
- [`-Xsource-features:case-apply-copy-access` and `-Xasync` (`agent/xflags`)](#-xsource-featurescase-apply-copy-access-and--xasync-agentxflags)
- [`-Ykind-projector`: kind-projector's type-lambda syntax (`agent/kindproj`)](#-ykind-projector-kind-projectors-type-lambda-syntax-agentkindproj)

Syntax that can be parsed (or desugared):

- packages / imports. **A `package` clause opens only the package it names** (SLS 9.2): a qualified `package p.q` opens `p.q` alone — neither `p`'s classes nor its subpackages are visible. A nested `package p { package q { … } }` opens both. The root is always consulted last, so a qualified reference `p.X` from `package p.q` resolves. The difference is observable: seen from `package slick.dbio`, `cats` is the top-level `cats`, not `slick.cats` (`agent/proj`). **The last-resort fallback to packages that are not open has been removed** (`agent/tail6`) — because that closed the gap it was masking (the right-hand side of a default argument being typed at the call site)
- objects / classes / traits / case classes. **Auxiliary constructors** `def this(...) = this(...)` (the head of the chain is `this(...)`; `super(...)`, or a `this` after a statement, is a diagnostic). A subclass's `extends C(1)` makes the primary constructor call the parent ctor. `new Inner` for an inner class keeps `$outer` as the first argument of `<init>` even after ctor overload selection. **`copy(...)` on a case class** (positional / when some arguments are omitted the corresponding field of the object itself is the default / named arguments. At namer time the types of the ctor fields are not yet settled for `copy`, so the argument symbols of `copy` itself and `copy$default$N` are rebuilt in the typer phase, after field types have been resolved. It also works on the private runtime). **Default arguments on constructors** (`new C(1)` / `new C(y = 2, x = 1)` for `class C(x: Int, y: Int = 5)`): filling in default values for calls that omit trailing arguments is implemented by splicing the saved tree in place, rather than going through the default getter of an ordinary `def` (which is unusable at call sites that have no `this`). **That tree is typed in the scope where it was written** (`agent/tail6`; `Checker::record_default_scope` / `type_default_rhs_here`) — typing it at the call site would mean the defining file's imports have no effect and even the class's own members become visible. A constructor default sees neither the members of the class nor preceding ctor arguments (`class Pair(a: Int, b: Int = a)` is `not found: value a` in nsc too). **Reordering via named arguments works for `new C(...)` as well** (constructor overloads are narrowed by parameter name first, then decided by type)
- `val` / `var` / `def` (nested `def`s are parsed)
- **Expression statements in a template body** (`class A { println("ctorA") }`). Exactly as in SLS 5.1 / 5.3, they run as part of the primary constructor for a class, of `$init$` for a trait, and of module initialisation for an `object`, **interleaved in declaration order** with `val` / `var` initialisation. The same holds for early `require(...)` / `assert(...)`, for `if` / `match` / `try` / loops / lambdas, and inside the bodies of `case class`es, local classes, anonymous classes and member `object`s. See the "Expression statements in a template body" section for details
- Parameters, lambdas (typed / inferred from the expected type), blocks. **Placeholder `_`** (nsc's `withPlaceholders`): `_ + 1` / `_.abs` / `f(_)` / `xs.map(_ + 1)` / Function2 `_ + _` / nested `_.map(_ + 1)`, plus **typed `_ : T`** (`(_: Int) + 1` / `(_: Int) + (_: Int)` / `(_: Int).abs` / `xs.map((_: Int) + 1)`). The lexer turns `_:` into `Ident("_")`, so in expression position it becomes the same placeholder as an Underscore. A bare `(_: Int)` is `unbound placeholder parameter`. `xs.map(_ : Int)` is not wrapped, exactly as in nsc: an Int is passed to map and it is a mismatch. The existing wrapping for unary and Function2 is left alone. **Method application sections** `f(_, x)` / `f(_, _)` take their parameter types from the callee's signature even with no expected type (under the same condition as nsc: only when the callee is a single non-generic method. `poly(_, 3)` and the overloaded `"abc".substring(_)` remain `missing parameter type for expanded function`). Synthetic parameters are ordered in source order (`two(_, _)` is `(a, b) => two(a, b)`). **The body of a literal is checked against the result of the expected type** — `xs.foreach((x: Int) => x + 1)` is value discarding, `fl((x: Int) => x)` is a numeric widening to `Int => Long`. A literal that writes its parameter types is typed before the expected type for the sake of overload resolution, so that part is done on the `adapt` side. Function **values** are out of scope: `val h: Int => Int = …; fu(h)` is a `type mismatch`, as in nsc
- `if` / `else`, `while`, `do { ... } while (cond)`
- `try` / `catch` / `finally` (catch is `{ case ... }`. Both `try/finally` and `try/catch/finally`. The finally block runs both on normal completion and on an exception, including a throw from a catch. A JVM exception table is emitted. The parser does not drop `finally`)
- `match` (constructor patterns, literals, wildcards, stable identifiers for Java enum constants `Thread.State.NEW`, `x @ Pat` bindings, `case null`, nested extractors `case P(v) :: t`. If no case matches, `scala.MatchError`)
- for-comprehensions (desugared to `map` / `flatMap` / `foreach` / `withFilter`. On the private runtime, `List.withFilter` is an eager `List`. Under `--scala-library` it is `scala.collection.WithFilter[+A, +CC[_]]`, whose `map[B]` returns `CC[B]`; `Option.withFilter` is `Option$WithFilter`). A value definition `q = e` becomes a `val` in the lambda body — it is **not a generator**, so the generator before it still takes `map` innermost. A **guard following** a value definition needs nsc's tupling, so it is reported as a diagnostic
- apply / select / infix (an operator ending in `:` is right-associative and its receiver is the right operand; `1 :: Nil` becomes `Nil.::(1)`). Assignment `xs(i) = v` becomes `xs.update(i, v)`, as in nsc. For a non-assignment `c(1)` with no `apply`, a diagnostic is reported (it is not silently turned into `update`)
- Literals, tuples
- Named and generic types (`Array[String]`, `def id[T](x: T): T`, and so on). The infix type `A Either B` is `Either[A, B]`. Applied syntax such as `Map[K, V]` is taken as written. **Higher-kinded types** `trait Functor[F[_]]` / `class Box[F[_], A](val fa: F[A])`; concrete instances such as `Id[_]`. Kind mismatches (using `F[_]` in a proper position, using a proper type as a type constructor) are reported as diagnostics (not silently discarded). **`Array` has kind `* -> *`** (a source `Array[T]` becomes `Type::Array` and no type parameters end up on the symbol, so `kind_arity` special-cases it. `TC[Array]` is accepted, just as in nsc; `agent/asttype`). A wildcard type argument takes on the parameter's kind **only inside a type pattern** (`case o: TC[_]`); a `TC[_]` in an ordinary type position is rejected as in nsc. **Higher-kinded type members** `trait M { type F[_] }` and path-dependent application `m.F[Int]`; concrete instances in a subclass as `type F[X] = Id[X]` (or `List[X]`). Kind mismatches on members (binding a `type F[_]` with `type F = Int`, and the converse) are reported as diagnostics. **Higher-kinded type members in refinements** `M { type F[X] = Id[X] }` and their application. **Structural type lambdas** `({ type L[a] = Either[String, a] })#L` as a type constructor argument, including one that captures an enclosing type parameter (`Monad[({ type L[X] = Reader[R, X] })#L]`); a lambda, a named higher-kinded alias for the same body, and a plain class constructor are one type (`agent/typelambda`, `docs/cats.md`). kind-projector's `λ[α => …]` and `*` are a compiler *plugin*, not Scala, and stay rejected as nsc rejects them. **HK bounds** `type F[_] <: Bound` (a proper bound; `type F[_] <: List` is `takes type parameters` as in nsc). **Bounds in refinements** `{ type A <: Int }`. Nullary `type A <: T` on a class or trait is still unimplemented and is reported as a diagnostic. **Nested type projections** `Outer#Inner#X` / `Holder#Inner#T`. The illegal `Int#X` and the abstract `B#U#T` (no such member) are `is not a member`, as in nsc. **Members of a projection are re-read from the prefix type**: if the `B` of `A#B` is a nested class of an ancestor of `A`, then abstract type members appearing in the types of `B`'s members are read with the definitions `A` supplies (the `def session: S` of `Sub#Ctx` is `Sess` through the `Sub` that carries `type S = Sess`). This re-reading is as-seen-from, not a constraint, so `A#B` is plain `B` both for subtyping and for display (`agent/proj`)
- 2.13 early field defs: `class C extends { val x = 1 } with T`. `x` is written into the field before the parent ctor / trait `$init$` (as in nsc). Anything other than a concrete field (`def` / statement / abstract val) is `only concrete field definitions allowed in early object initialization section`, as in nsc. A `this` inside an early block is `this can be used only in a class, object, or template`
- A subset of SIP-23 literal types: `val x: 1 = 1`, `def f(x: 1): Int`. Literals in expressions have constant types (`1 <: Int`). A mismatch `val y: 1 = 2` is a type mismatch. The classfile pickle uses nsc's `CONSTANTtpe` + `LITERALint` (scalac 2.13.16 can typecheck `def f(x: 1)` / `val one: 1` through `-cp`)
- `scala.Dynamic`: `d.foo` becomes `selectDynamic("foo")`, `d.foo(args)` becomes `applyDynamic("foo")(args)`, `d.foo = v` becomes `updateDynamic("foo")(v)`, `d.foo(a = x)` becomes `applyDynamicNamed("foo")(("a", x))`. `import scala.language.dynamics` (or `-language:dynamics`) is required. Under `--scala-library` it runs against the jar's `scala/Dynamic`
- A subset of XML literals (2.13): `<a>t{e}</a>` / `<a/>` / `<a b={e} c="t"/>` / `<a xmlns:p="u" p:b={e} c="t"/>` / `<p:a xmlns:p="u"/>` / `<p:b xmlns:p="u">t</p:b>` / `<a><!--c--></a>` / `<a><![CDATA[x]]></a>` / `<a><?pi t?></a>` / `<a>&amp;</a>` / `<a>&#65;</a>` (elem / text / splice / unprefixed attributes / `xmlns:p` and prefixed attributes `p:b` / prefixed element names / comments / CDATA / PIs / the predefined entities `&amp;` `&lt;` `&gt;` `&quot;` `&apos;` / numeric `&#N;` `&#xN;`). Attributes use the same `UnprefixedAttribute` / `PrefixedAttribute` chain and `NamespaceBinding` as nsc. A prefixed `Elem` carries the string in `prefix` and the local name in `label`. Comments / CDATA / PIs are `scala.xml.Comment` / `PCData` / `ProcInstr`. Predefined entities are `EntityRef`, numeric references are `Text`. The lexer splits `><!--` into `>` and `<`. Unknown entities are reported as diagnostics. `scala-rs run` adds any scala-xml jar it can find to `java -cp`
- `scala.Enumeration`: `object Color extends Enumeration { val Red, Blue = Value }` (several `val`s get consecutive ids). Under `--scala-library` it runs against the jar's `Enumeration`, and the 4 overloads of `Value` (`Value` / `Value(i)` / `Value(name)` / `Value(i, name)`), `values: ValueSet` (`toList` / `filter` / `size` / `contains`), `withName` / `apply` / `maxId`, `Value.id` / `toString`, and the stable-identifier pattern `case Color.Red =>` are all usable. Everything from `values` on down is read from the jar's `ScalaSignature` (`agent/uniteq`)
- Conformance: the **inheritance relations of the collections** (`Vector[A] <: IndexedSeq[A] <: Seq[A] <: collection.Seq[A] <: Iterable[A] <: IterableOnce[A]`, `List` / `LazyList` / `Queue` / `Range` / `ArraySeq`, `Set[A] <: Iterable[A]`, `Map[K, V] <: Iterable[(K, V)]`, and likewise on the mutable side) are wired up with type arguments from a single table in `crates/typer/src/prelude_hier.rs`. **An annotated type** conforms just like the type underneath it (`Node` for `Node @uncheckedVariance`). **A module's `.type`** is the module's own type (`Some(Nil): Some[Nil.type]`). For a class with contravariant parameters, the lub takes the glb for exactly those parameters (the lub of `Act[+R, -E]` is `Act[R lub R2, E glb E2]`). The lub of a type parameter follows its upper bound. The parent constructor arguments of `extends Base[T](y)` are read with the type arguments written in the `extends` clause. Against `type Self >: this.type <: Nd`, `this` conforms (the lower bound `this.type` is re-read on the subclass side, as in `class Leafy extends Nd { type Self = Leafy }`) while an arbitrary `Nd` does not
- The language flags `implicitConversions` and `postfixOps` behave as in nsc 2.13. A user-defined `implicit def` / `implicit class` produces a **warning** without the import or `-language:implicitConversions`. Postfix `42 bang` / `42 abs` produces a **warning** without `import scala.language.postfixOps` (or `-language:postfixOps`), which is an error under `-Xfatal-warnings`
- The common shapes of existential types: `List[_]`, `T forSome { type X }`, methods taking a `List[_]`, the bounded `List[_ <: AnyRef]` and `List[X] forSome { type X <: AnyRef }` (named quantification is lowered to `BoundedWildcard` so that the existing pickle/erase paths are reused). Wildcards erase to the equivalent of Object. A nested `List[_ <: List[_]]` is pickled as an EXISTENTIALtpe on the hi-bound side. `p.Inner forSome { val p: Outer }` is packed into `Outer#Inner` and runs. Other `forSome { val … }` forms are reported as diagnostics (not silently discarded)
- **ScalaSignature** on compiled classes/objects (the class attribute `ScalaSig` marker plus a pickle subset in `RuntimeVisibleAnnotations`). It is visible under `javap -v`. Separate compilation via `-cp` works as far as our own unpickler can read. It is not a full nsc pickle, but the wire format is the same as nsc's (nentries, tag/len, big-endian Nat, SID-10 as `0x7f→0`). `val`s / `def`s with parameters / type parameters `id[T]` / `new` and ctor fields of a `case class` / **companion `apply` `Point(3, 4)` (the term `Point` / `MODULE$`)** / **extractor `unapply` (`p match { case Point(a, b) => … }`)** / `def`s on an object / **`List[_]` (EXISTENTIALtpe)** / **`List[_ <: AnyRef]` (the hi bound of a quantified TYPEsym)** / **`@deprecated("msg", "2.13.0")` (SYMANNOT + LITERALstring)** / **Java `@Deprecated` (SYMANNOT + TypeRef(java.lang, Deprecated); scalac's `-deprecation` looks at annotations on methods)** / **`this.type` (THIStpe as a method result)** / **`Int @unchecked` (ANNOTATEDtpe)** / **`val one: 1` and `def lit(x: 1)` (CONSTANTtpe + LITERALint)** / **`List[_ <: List[_]]` (nested EXISTENTIALtpe)** / **`A with B { def f: Int }` (REFINEDtpe)** / **`@Ann(foo)` / `@Ann(c.x)` / `@Ann(3)` / `@Ann(this)` / `@Ann(classOf[Int])` / `@Ann(ident(1))` / `@Ann(this.x)` / `@Ann(super.foo)` / `@Ann(ident(ident(1)))` / `@Ann(foo = 1)` (TREE Ident/Select/This/Super/Apply + literals / LITERALclass Constant, including nested Applies and Select qualifiers other than Ident; the named `@Ann(foo = 1)` becomes a positional Constant, as in nsc)** / **`def join(xs: String*)` (VARARGS + `<repeated>`)** / **the `Ordered` erasure bridge (BRIDGE)** / **`type T = Int` (ALIASsym; 2.13 has no ALIAStpe)** / **nested classes and objects (`class Outer { class Inner }`, `object Support { trait Rows }`), declared as CLASSsym / MODULEsym entries of the enclosing class as well as in their own class file — nsc resolves `Outer.Inner` as a member of `Outer`'s signature and never opens `Outer$Inner.class`, so a parent list that mentioned a nested Scala trait used to be unreadable** are all emitted in a form scalac 2.13.16 can read (an object is CLASSsym+MODULE + MODULESYM, and the class pickle also carries the companion's MODULESYM; EXTMODCLASSref for packages (`hklib` / `slick/ast`) and for scala / java.lang, with `<empty>` only for the default package; POLYtpe puts restpe first; a val is a NullaryMethodType getter; case classes get CASE / CASEACCESSOR; user types are EXTREFs owned by **their own package**; `Option` / `TupleN` / `FunctionN` / `List` are TypeRefs owned by the scala / `scala.collection.immutable` module plus type arguments; Flags are emitted by running nsc's raw long through `rawToPickledFlags`). A signature that outgrows the 65535 bytes of one `CONSTANT_Utf8` (JVMS 4.4.7) is split across SID-10's `ScalaLongSignature`, as nsc does. It does not claim to be a full pickle: `uncurry` has already flattened `paramss` when the pickle is written, so a curried or implicit parameter section is read back as one list. The remaining gaps are in [not-implemented.md](not-implemented.md) and [slick-testkit.md](slick-testkit.md)
- **`Signature` (JVMS §4.7.9)** on classes, methods, constructors and fields: the generic type information erasure throws away, which is what `Class#getGenericSuperclass` / `#getGenericInterfaces`, `Method#toGenericString` and `Field#getGenericType` read. Formal type parameters with their bounds (`<A:Ljava/lang/Object;B::Lscala/collection/immutable/Seq<TA;>;>`), type arguments (`Wrapper<[I>` for `Wrapper[Array[Int]]`, `Wrapper<Ljava/lang/Object;>` for `Wrapper[Int]` -- a primitive type argument is `Object`, not its box, as in nsc), type variables (`TT;`), wildcards (`*` / `+Hi` / `-Lo`), `FunctionN` / `TupleN` / by-name (`Function0`) / repeated (`Seq`), and the superclass and interface list in the order the class file writes it. nsc builds these in `erasure.javaSig` `enteringErasure`; this compiler's erasure phase rewrites symbol types destructively, so `crates/backend/src/sig.rs` runs from the driver between the pickler and erasure and records a candidate per symbol. The emitter attaches one only when it differs from the descriptor **and** erases back to it exactly (JLS 4.6: a type variable erases to its leftmost bound), so every place where this compiler's erasure and nsc's disagree loses the attribute rather than emitting a claim the descriptor contradicts. `SCALA_RS_SIG_DEBUG=1` prints the rejects
- **`ConstantValue` (JVMS §4.7.2)** for `@SerialVersionUID(n)`: nsc's `private static final long serialVersionUID`, which is where `java.io.ObjectStreamClass.lookup` reads the id. The argument is constant-folded (`10L + 3L`)
- **`InnerClasses` (JVMS §4.7.6) and `EnclosingMethod` (§4.7.7)** on compiled classes/objects. Previously neither was emitted at all, so `getClass.getSimpleName` returned `Main$Circle` instead of `Circle`, `isMemberClass` was always `false`, and `getEnclosingClass` / `getDeclaringClass` were always `null` (all of these read this attribute). Nested classes / traits / objects (with `class Circle extends Shape` and both of them directly under `object Main`) carry both a self entry (`outer_class_info` = the enclosing class, `inner_name` = the simple name in the source) and the **other** nested classes that appear in that classfile's own constant pool (`implements` / `checkcast` / the types of fields and of `$outer`, and so on). In addition, nested classes and objects that the class declares directly inside itself are listed **unconditionally**, even when they are not actually referenced (this matches the behaviour of real scalac for `Outer` / `Outer$Level1`, confirmed with `javap -v`). Local classes and anonymous classes (`new Shape { ... }`) leave `outer_class_info` at 0 (so `isMemberClass` is `false`) and emit an `EnclosingMethod` instead. `inner_name` is 0 only for anonymous classes (so `getSimpleName` is the empty string). `access_flags` are the modifiers **as written in the source** (`public`/`private`/`protected`; `static` meaning it has no `$outer` field; `final`), and are a different thing from the classfile's own `access_flags` (a module class's own `final` is implicit and is not emitted; a value class's `final` is emitted even though it is not written, because it comes from `extends AnyVal`). The static forwarder that `object Main` itself generates for nested objects (the "mirror" class `Main`, emitted when the `object` has a `def main`) is not itself nested and so has no self entry, but, like real scalac's mirror class, it unconditionally lists the direct members of the object it is linked to. The companion of a case class, and the companion holding a value class's `$extension`, go through the same path as any ordinary nested module class. The **disambiguating numeric suffix** on local classes, such as `LocalC$1`, exists in nsc but not yet in scala-rs (an existing gap unrelated to this attribute). Fixture prefix `inner` (`crates/cli/tests/innerclasses.rs`)
- `s"..."` / `f"..."` / `raw"..."` string interpolation. `f"$n%02d"` is lowered to `String.format`. `raw` does not interpret escapes. Date/time (`%t`/`%T`), argument indices and the relative `% <` are reported as diagnostics. Under `--scala-library`, custom interpolators (the `q"a$x"` of `implicit class Q(sc: StringContext) { def q(args: Any*) }`) are desugared to `StringContext.apply(parts*).q(args*)` and run. On the private runtime, anything other than `s`/`f`/`raw` is a diagnostic
- Context bounds `T: ClassTag` / `T: Ordering` / `T: scala.reflect.ClassTag` (method type parameters) and **class type parameters** `class C[T: Ordering](x: T)`. As in nsc, they desugar to implicit evidence `C[T]` (for a class, an extra implicit clause on the primary ctor). A `: C` or `<%` on a trait is `traits cannot have type parameters with context bounds ': ...' nor view bounds '<% ...'`, as in nsc. Missing evidence is `no implicit`. Under `--scala-library` this reads the jar's `scala.math.Ordering` from the classfile and works by linking to the companion's `implicit object Int` (`Ordering$Int$.MODULE$` / InnerClasses) and to `ClassTag`. A generic `Array[T].length` is lowered to the jar's `ScalaRunTime.array_length`
- `lazy val`. A member becomes `bitmap$0` plus accessors; **a method-local one becomes a `scala.runtime.LazyRef` (with dedicated cells such as `LazyInt` for primitives, and `LazyUnit` for `Unit`) plus a lifted accessor**, and the declaration site only creates the cell. The initialiser runs at most once, on the first read, under the cell's monitor (the same shape as nsc's `lazyvals` phase). Inside a block, only a `lazy val` may be forward-referenced
- implicit val / def (local, imported, package object, companion), implicit parameters, implicit conversions in scope. This includes explicitly passing a second parameter clause, `foo(x)(y)`. When there are several candidates, the nsc-style **more-specific** rule applies (a subtype result type, or an origin whose defining class is a subclass). When type and origin disagree (a more specific implicit on the parent versus a less specific local defined on the child) the result is `ambiguous implicit`. Two candidates of the same type are ambiguous. When the target type is `A => B` with `A <: B`, an identity view is synthesised, as in nsc (the call site of a view bound). **implicit classes** (in an object or class body; the `2.twice` of `implicit class Rich(n: Int) { def twice: Int }`). **`implicit class` in a package object** (from another compilation unit in the same package, or via `import pkg._`; IMPLICIT in the pickle. A top-level `implicit class` is `` `implicit` modifier cannot be used for top-level objects ``, as in nsc. Without the import, the enrichment is not visible). **A value of a class extending `Function1` is an implicit conversion too** (nsc's rule of "does the candidate's type conform to `From => To`". Since `scala.<:<[-From, +To] extends (From => To)`, an `implicit ev: P <:< Q` converts a `P` into a `Q` and the application is `Function1.apply`. Implicit methods that take no arguments (`<:<.refl`) are not made into views)
- `@tailrec` (a `def` that is not tail-recursive is an error, in nsc's style. Tail recursion in an object is accepted and runs. There is no while-loop transformation. **For a method that takes no parameter list**, a recursive call is a bare `Select` with no `Apply`, so for a declaration with empty `paramss` that too counts as a call — the shape of slick's `NominalType.sourceNominalType`. In non-tail position it remains `a recursive call not in tail position`, as before. `agent/asttype`) / `@deprecated` (annotations with arguments are put into the pickle's SYMANNOT; compilation is not broken) / Java `@Override` (a method that really does override is accepted; otherwise `overrides nothing`) / Java `@Deprecated` (emits `Ljava/lang/Deprecated;` in the method's `RuntimeVisibleAnnotations`; the pickle is `SYMANNOT` + a TypeRef for `java.lang.Deprecated`. Visible both to `javap -v` and to scalac's `-deprecation`) / user-defined `StaticAnnotation`s (the Ident/Select/This/Super/Apply, literal, classOf, named Constant and named TREE arguments of `@Ann(foo)` / `@Ann(this)` / `@Ann(classOf[Int])` / `@Ann(ident(1))` / `@Ann(this.x)` / `@Ann(super.foo)` / `@Ann(ident(ident(1)))` / `@Ann(foo = 1)` / `@Ann(foo = this.x)` / `@Ann(foo = bar)` are pickled as TREE or Constant; named arguments are rewritten to positional ones before pickling, as in nsc) / `@implicitNotFound("…")` (a missing implicit gets exactly that wording, as in nsc; `${A}` is the type argument) / `@switch` (`(n: @switch) match`. Dense Ints become a `tableswitch`, sparse ones a `lookupswitch`. A match that cannot be turned into a switch gets nsc's warning `could not emit switch for @switch annotated match`). `@inline` / `@noinline` are merely stored as annotations; no inlining is performed. Real scalac 2.13.16 does not validate their placement at all (put them on a val, var, class, type — anything — or put both on at once, and it does not even warn: this is information only the `-opt:inline:...` bytecode optimiser reads, and it is unrelated to the typer), so scala-rs does not validate it either. `@volatile` / `@transient` become the classfile's `ACC_VOLATILE` / `ACC_TRANSIENT` (visible in javap). `@native` on a method emits `ACC_NATIVE` and no body is attached (no `.so` is linked; a body, or the annotation on a val, is a diagnostic)
- Non-local `return` (from a nested lambda or `foreach` to the enclosing named method; nsc-style `scala.runtime.NonLocalReturnControl`). A `return` in a nested `def` belongs to that def itself. A `return` from a class constructor is `return outside method definition`
- `eq` / `ne` (AnyRef reference equality) and `synchronized` (monitorenter / monitorexit; the body is evaluated while holding the lock)
- `asInstanceOf[T]` / `isInstanceOf[T]` (genuinely generic methods on `Any`, with a type parameter `T0`. Primitives get a `checkcast` to the box plus an unbox call, `String` and class types get a `checkcast`, and erased or unbounded targets (`Any` / `AnyRef` / a type parameter) need no cast. Because `x.asInstanceOf[T]` carries the concrete `T` that was assigned only on the outer `TypeApply` node, both the erasure phase and the backend must read it from that outer node). `null` resolves members as `AnyRef`/`Any` (so `null.asInstanceOf[String]` works). An unbounded type parameter `T` also resolves members as `Any` (so `x.asInstanceOf[AnyRef]` for `x: T` works)
- The companion constants of `scala.Int` / `Long` / `Short` / `Byte` / `Char` / `Double` / `Float` (`Int.MaxValue` / `MinValue`, `Double.NaN` / `PositiveInfinity` / `NegativeInfinity` / `MinPositiveValue`, and so on). They are really nullary methods on the companion object (`scala/Int$.MODULE$.MaxValue()` etc.), and only under `--scala-library` (the real jar is required; on the private runtime they are a diagnostic)
- The constructors of `java.lang.Throwable` / `Exception` / `RuntimeException` (`()` / `(String)` / `(String, Throwable)` / `(Throwable)`) and `getMessage` / `getLocalizedMessage` / `getCause` / `initCause` / `printStackTrace`. Previously only the 0-argument ctor "worked", and only by accident (because it had no arguments). Since these are the real `java.lang.*`, they are usable under `--no-scala-library` too
- `Array(1, 2, 3)` / `arr(0)` / `arr.length` / `arr.update` under `--scala-library` (the jar's `scala.Array$` + `ClassTag`; there is no companion apply on the private runtime)
- Overloading: `def`s with the same name are chosen nsc-style by argument type and arity (the more specific parameter type wins). Ambiguity is `ambiguous overload`, no candidate is `no matching overload`. **In value position, only candidates that take no arguments are kept** (SLS 6.26.3). A `val` is not a method type in the first place, so `object Library { val == = new SqlOperator("=") }` is not ambiguous with the inherited `Any.==(x: Any)` and can be read as a value (this is also how the extractor in `case Library.==(a, b)` is found). Identical candidates that appear twice because of duplicated inheritance paths are treated as a single member
- Companion `apply` / `unapply`: only the synthetic members of a case class are filled in with the constructor's signature. **An `apply` written by hand on an ordinary class's companion (including one with default arguments, or with an implicit clause following varargs) is left as it is**
- **A clause following varargs** (`def f(ch: Node*)(implicit t: TypedType[T])`): even when the varargs are passed zero arguments, as in `f()`, the following implicit clause is still filled in properly
- Turning `{ case … }` into an anonymous class in a position expecting `PartialFunction[A,B]`. `isDefinedAt` / `apply` / `applyOrElse` work. Under `--scala-library`, so does `List.collect`
- **Pattern-matching anonymous functions** (nsc's "pattern-matching anonymous function"): `xs.map { case (s, t) => … }` / `xs.collect { case … }` / `catch { case … }` pass the scrutinee type `A` from the expected type `A => B` / `PartialFunction[A, B]` into the patterns, and the result type is the lub of the individual case bodies. The old behaviour of typing the body as `Any` — collapsing to `List[Any]` — when the callee's result type parameter (`map`'s `B`) was not yet decided has been fixed. `if` and `match` likewise take the lub of the branches when the expected type is `Any` or an undetermined type parameter. Constructor patterns propagate the scrutinee's type arguments (`case Box(v)` against a `Box[Int]` gives `v: Int`; fields erased to a type parameter are unboxed / checkcast)
- `private[this]` and `protected[C]` (`protected[pkg]` has the same qualification) are enforced in the typer. `private[this]` rejects anything but a `this` prefix (i.e. other instances). `protected[C]` allows access from inside C and from a subclass via `this`
- **lambda-lifting** of nested `def`s (a synthetic method capturing locals; using one as a value and recursive calls from a lambda both work)
- Default arguments and by-name parameters (`=> T`). Defaults are emitted into the classfile as `{method}$default$n` getters, exactly as scalac does (1-based, taking the preceding parameters). The call site calls that getter rather than inlining the AST, so separately compiled code can use them too
- **Bounds on method type parameters**. **Lower bounds `[B >: A]`**: as in `def ::[B >: A](elem: B): List[B]`, the **lub** is taken of the `B` inferred from the argument and the actual type of `A` as seen from the receiver. `Circle(1) :: Rect(2, 3) :: Nil` is `List[Shape]`, not `List[Circle]` (`SymbolTable::lub` walks the parents to find the common base type). A user-defined `class Box[A] { def widen[B >: A](other: B): Box[B] }` infers through the same path. Varargs likewise take the lub of all arguments, so `List(Circle(1), Rect(2, 3))` under `--scala-library` is `List[Shape]`. **Upper bounds `[A <: Named]`**: both inferred and explicit type arguments are checked, and reported with nsc's wording `inferred type arguments [Int] do not conform to method f's type parameter bounds [A <: Named]` (`type arguments [Int] do not conform to …` when explicit). A value of `[A <: Named]` may be used where a `Named` is expected
- View bounds `T <% Ordered[T]` / `T <% Ordered[Int]` (method type parameters) and **class type parameters** `class C[A <% Ordered[A]](x: A)`. As in nsc, they desugar to implicit evidence `T => V` (for a class, an extra implicit clause on the primary ctor). Missing evidence is `no implicit`. For a higher-kinded type parameter, scalac 2.13.16 rejects every spelling of `F[_] <% V` (`type F takes type parameters`); scala-rs gives the same diagnostic. No Scala-3-style encoding is attempted
- `extends App` / `DelayedInit`. `object Main extends App { println(...) }` moves the constructor body into `delayedInit` and starts from `App.main`, as in nsc. A class inheriting `DelayedInit` without App also gets its `delayedInit` hook called
- **Named arguments** (reordered at the call site). They work for methods, for a companion's `apply`, for a case class's `copy`, for **constructors `new C(b = 2, a = 1)`**, for **overloaded calls**, and with varargs (`f(a = 1)` / `f(a = 1, 2, 3)` for `def f(a: Int, rest: Int*)`). Reordering follows the same rules as nsc's `NamesDefaults.removeNames`: **a named argument that is in its own position permits positional arguments after it** (`f(a = 1, 2)` is accepted, while `f(b = 1, 2)` is `positional after named argument.`). As in nsc, overloads are first narrowed by parameter **name**, and the argument types decide only when names alone are not enough. The diagnostics use exactly real scalac's wording (`unknown parameter name: q` / `parameter 'c' is already specified at parameter position 2` / `positional after named argument.`) and, as in nsc, only one is emitted per call (follow-on messages such as "not enough arguments" are cascades and are suppressed). **They also work when the callee is an `object` applied directly** (`html.dropdown(value, right = true)`, the shape every Twirl template has): the members of an `object` live on its module *class*, while a reference to it carries the module *value*'s symbol, which has none, so the reference has to be followed through its `ModuleRef` before `apply`'s parameters can be found (`agent/namedargs`; all 53 of gitbucket's "named arguments (method parameters not resolved)" were this). **Reordering does not change the evaluation order**: SLS 6.6.1 evaluates arguments left to right *as written* and matches them to parameters afterwards, so a call whose names moved anything binds its arguments to locals in front of the call, as nsc's `NamesDefaults.transformNamedApplication` does — `crates/typer/src/named_eval_order.rs`, a pass over the typed tree. A literal, a `this`, an immutable name and a function literal stay where they are (moving them cannot be observed), an argument to a **by-name** parameter is never lifted (the call site does not evaluate it at all), and a receiver that computes is bound first. What remains unnamed is the hand-written prelude: `List(1,2,3).mkString(sep = "-")` and `map.updated(key = k, value = v)` are still "named arguments (method parameters not resolved)", because `prelude::method` declares parameter *types* only. Members completed from a pickle — scala-library's or a `-cp` classfile's — do carry their names and work
- Mixin of traits with concrete members (interface `default` methods plus nsc's `m$` statics, and forwarders in linearisation order). Forwarders are emitted for both `class` and `object`. The runtime representation of a trait's `val` / `override val` / `var` is covered in the "Trait mixin" section
- **The synthetic members of a case class / case object**: a case class gets `toString` / `equals` / `hashCode` / `canEqual` / `productPrefix` / `productArity` / `productElement` / `productElementName`. A **case object** gets, on the module class side, the same constant-folded `toString` as nsc (`Foo`, not `Foo$@1a2b3c`) / `productPrefix` / `hashCode` (`"Foo".hashCode`) / `productArity` (0) / `canEqual` / `productElement`. `equals` stays singleton reference equality (inherited from `Object`), as in nsc. A hand-written definition wins
- **Case classes and case objects are `scala.Product with java.io.Serializable`** (when linked against the jar). Both `val p: Product = P(1, 2)` and `List[Product]` are accepted, and `productIterator` / `productElementNames` are inherited from `Product`, as in nsc. **The synthetic companion extends `scala.runtime.AbstractFunctionN`**, so `P.tupled` / `P.curried` / `val f: (Int, String) => P = P` work. See the "Making case classes `Product`s" section for details
- **A diagnostic for reassignment to a `val`** (both `val x = 1; x = 2` and `d.v = 5` (a trait's `val`) give nsc's `reassignment to val`). Java fields and compiler-generated synthetic terms are out of scope
- Inner classes (`$outer`) and nested objects. Anonymous classes `new Trait { def f = ... }` and `new { def x = 1 }` (a synthetic classfile; the type is `$anon$N`, not a refinement)
- **`object`s that are members of a class or trait**. Unlike a top-level `object`, they are not static singletons: there is **one per enclosing instance**. As in nsc, a `$outer` field and an `<init>` taking the enclosing instance are emitted (with no `MODULE$` and no `<clinit>`), and on the enclosing template side a `private volatile <name>$module` field and a `<name>()` accessor that creates it on first reference. When it is a trait member, the interface declares `<name>()` abstract and the implementing class holds the field and the accessor (the same shape as a mixed-in `lazy val`). An `object` inside a non-static `object` is likewise non-static (the `N` of `class Outer { object P { object N } }` has `$outer: Outer$P$`). The companion of a `case class` nested in a class is treated the same way, and `copy` passes its own `$outer` to the new instance. See the "Nested types" section for details
- Classes defined inside a method body (anonymous classes `new T { … }` and **local `class`es / `object`s**) **capturing the enclosing method's parameters and locals**. In the same shape as nsc, a public final field `x$1` is emitted per free variable, plus extra constructor arguments appended at the end. Each instance method reads those fields back into local slots at its start, so reads and writes of a captured `var` through `scala.runtime.*Ref`, and double capture by a lambda inside the anonymous class (`$captured$N`), both keep working through the existing paths. A class inside a method also gets a `$outer`, and members of the enclosing class are read through the `$outer` chain
- eta-expansion `foo _`, and unapplied methods in positions expecting a FunctionN (`xs.map(inc)`). Nested parameter lists become one list plus a closure in **uncurry**. SIP-21 SAM: a lambda or an unapplied method conforms to `Runnable` / `java.util.Comparator[Int]` / `java.util.function.Function[A,B]` (a single abstract method). To a non-SAM type it is a type mismatch (no silent wrapping). Passing `def go(): Unit` to a `Runnable` without `_` auto-applies and is a mismatch, as in nsc. The synthetic class does not use invokedynamic, just like the existing anonfuns
- **Curried constructors**: `new C(1)(2)` for `class C(a: Int)(b: Int)`. As with `extends A(1)(2)`, there is only one `<init>` on the JVM, so the argument lists are flattened before resolution. An explicitly given implicit clause (`new K[B]("s")(ev)`) is **not searched again**. Named arguments in a later clause (`new C(1)(c = 3, b = 2)`) are accepted too. A case class's `copy(…)(…)` is rewritten into this constructor call (`agent/tail4`)
- **Accessors for constructor arguments**. Both `class C(val x: Int)` and **the first argument list of a `case class`**, which becomes `val` without the keyword, turn into a public accessor `x()` implementing the parent's abstract member (when the parent erases `def value: T` to `()Object`, a bridge is emitted too). The second and later argument lists stay private state, as in nsc. A `var` parameter gets both `x()` and `x_$eq(v)`
- **`FunctionN.tupled` / `curried` (arity 2 through 22) and `scala.Function.untupled` (2 through 5)**. These are the default methods of `scala/FunctionN` and `scala/Function$`, so they are **only available when linked against the jar** (a diagnostic under `--no-scala-library`). Along with that: if the result of a method with no argument list is a function, its argument list is the function's (`def g: Int => Int; g(3)`), and `f(1)(2)` on a curried **function value** is two `Function1.apply` calls (unlike curried methods, this is not flattened)
- **`+=` / `++=` on `scala.collection.mutable.Builder`** (default methods on `Growable`; they return `this.type`, so the receiver's type comes straight back). Only when linked against the jar
- `super` and qualified `this` (`Outer.this`). A trait's `super` goes through the trait's `m$` static for a concrete class, or through `p$q$T$$super$m` for a stackable `abstract override`
- **Override conformance checking** (SLS 5.1.4 / 5.2.6; `crates/typer/src/override_check.rs`). Covariance of result types (`incompatible type in overriding`), invariance of parameter types (a difference makes it an overload; with `override` written, `method f overrides nothing.` plus the same `Note:` as scalac), whether the `override` modifier is required, a deferred redeclaration cancelling a concrete implementation below it, `final`, not being allowed to narrow visibility (`weaker access privileges in overriding`), that a `val` may override a `def` but not the reverse and that a concrete `var` may not be overridden, the number and bounds of type parameters, and **unimplemented abstract members** (`class X needs to be abstract.` / `object creation impossible.`). The wording is real scalac 2.13.16's, and the overridden member is echoed **as seen from the override site**. Members coming from the prelude and from pickles do not carry flags (`FINAL` / `DEFERRED`), so the `final` and unimplemented-member checks are **limited to members from source and from Java classfiles** (details and remaining items in the "Override conformance checking" section)
- Exhaustivity checking of matches over a `sealed` hierarchy (a missing case is a **warning**, an error under `-Xfatal-warnings`)
- Extractor `unapply` (`Option` / `Boolean` / `Tuple2`) and `unapplySeq` (`List` / `Seq` / `Vector` / `IndexedSeq` / `Array`, and varargs `_*`). Named extractor arguments (`Point(y = b, x = a)`)
- `AnyVal` value classes (one argument; instantiation erases to the underlying type, methods become `name$extension`). They can mix in universal traits that `extends Any`, and in positions requiring a reference (`Any` / that trait / a type argument / an array element) they are boxed with `new C(u)`. Pattern matching (`case x: C`) and `classOf[C]` / `asInstanceOf[C]` see the boxed class. `equals` / `hashCode` are synthesised from the underlying value (the equivalent of nsc's `equals$extension` / `hashCode$extension`)
- Part of Predef: `assert` / `require` / `???` / ArrowAssoc's `->` / `identity` / `locally` / `implicitly` / `any2stringadd` (`1 + "x"`) / String's `length` and `toInt` (`toLong` / `toDouble` are there too). Under **`--scala-library`** these link to the jar's `Predef$` / `StringOps` / `Predef$ArrowAssoc` / `Predef$any2stringadd`. Also, only when linked against the jar: `intWrapper` / `RichInt` (`abs` / `max` / `to` / `until`), `longWrapper` / `RichLong`, `doubleWrapper` / `RichDouble`, `floatWrapper` / `RichFloat`, `charWrapper` / `RichChar`, `StringOps`'s `*` / `take` / `drop` / `isEmpty` / `toUpperCase` / `toLowerCase` / `stripPrefix` / `split`, the varargs `apply` of `Map` / `Vector` / `List` / `Set` / `Seq` / `LazyList`, **`Either`** (`Left` / `Right` / `isLeft` / `isRight` / `map` / `flatMap` / `fold` / `getOrElse` / `orElse` / `swap` / `toOption` / `toSeq` / `contains` / `exists` / `forall` / `foreach` / `filterOrElse`, plus the `e` / `get` / `getOrElse` / `map` / `flatMap` / `foreach` / `exists` / `forall` / `toOption` / `toSeq` / `filterToOption` of the `LeftProjection` returned by `left`), and **`Try` / `Success` / `Failure`** (`Try(1)` / `isSuccess` / `isFailure` / `get` / `getOrElse` / `map` / `flatMap` / `filter` / `withFilter` / `foreach` / `orElse` / `recover` / `recoverWith` / `collect` / `toOption` / `toEither` / `failed` / `transform` / `fold`). `Option`'s `toList` / `toRight` / `toLeft` / `zip` / `collect` / `flatten` are also jar-link-only (`getOrElse` / `isDefined` / `nonEmpty` / `contains` / `exists` / `forall` / `filter` / `filterNot` / `orElse` / `fold` work on the private runtime as well). This slice links **the rest of ArrayOps** (`lengthIs` / `sizeIs` / `indexOf` / `copyToArray` / `iterator`; `zipWithIndex`/`knownSize`/`sizeCompare`/`filterNot`/`headOption`/`lastOption`/`partition`/`splitAt`/`span`/`find`/`contains`/`distinct` and everything before them are left alone), **the rest of StringOps** (`++` / `lengthIs` / `sizeIs` / `flatMap`; `iterator`/`sizeCompare`/`knownSize`/`appendedAll`/`prependedAll`/`>`/`>=`/`<=`/`compare`/`patch` and everything before them are left alone), and **`scala.collection.View`** (`List.view.map.toList`, `View.fill` / `View.iterate`. No private View classfile is emitted. LazyList/Iterator are left alone beyond what View calls require) against the same jar
- Part of Predef: `assert` / `require` / `???` / ArrowAssoc's `->` / `identity` / `locally` / `implicitly` / `any2stringadd` (`1 + "x"`) / String's `length` and `toInt` (`toLong` / `toDouble` are there too). Under **`--scala-library`** these link to the jar's `Predef$` / `StringOps` / `Predef$ArrowAssoc` / `Predef$any2stringadd`. Also, only when linked against the jar: `intWrapper` / `RichInt` (`abs` / `max` / `to` / `until`), `longWrapper` / `RichLong`, `doubleWrapper` / `RichDouble`, `floatWrapper` / `RichFloat`, `charWrapper` / `RichChar`, `StringOps`'s `*` / `take` / `drop` / `isEmpty` / `toUpperCase` / `toLowerCase` / `stripPrefix` / `split`, the varargs `apply` of `Map` / `Vector` / `List` / `Set` / `Seq` / `LazyList`, `Either` (`Left` / `Right`), and `Try` / `Success` / `Failure` (`Try(1)` / `map` / `getOrElse`). This slice links **the rest of ArrayOps** (`lengthIs` / `sizeIs` / `indexOf` / `copyToArray` / `iterator`; `zipWithIndex`/`knownSize`/`sizeCompare`/`filterNot`/`headOption`/`lastOption`/`partition`/`splitAt`/`span`/`find`/`contains`/`distinct` and everything before them are left alone), **the rest of StringOps** (`++` / `lengthIs` / `sizeIs` / `flatMap`; `iterator`/`sizeCompare`/`knownSize`/`appendedAll`/`prependedAll`/`>`/`>=`/`<=`/`compare`/`patch` and everything before them are left alone), and **`scala.collection.View`** (`List.view.map.toList`, `View.fill` / `View.iterate`. No private View classfile is emitted. LazyList/Iterator are left alone beyond what View calls require) against the same jar
- **The core members of `scala.collection.immutable.List`**. Under `--scala-library` they link to the real signatures of scala-library 2.13.16 (descriptors confirmed with `javap -s`). `map` / `flatMap` / `collect` / `zip` / `groupBy` / `sortBy` / `minBy` / `maxBy` / `foldLeft` / `foldRight` / `scanLeft` / `::` / `:::` / `+:` / `:+` / `++` / `:++` / `++:` / `updated` / `distinctBy` / `startsWith` / `endsWith` are **genuinely polymorphic** (they have a method type parameter `B`), so the element type can be tracked, as in `xs.map(x => "n" + x): List[String]`. Besides those: `filter` / `filterNot` / `take` / `drop` / `takeRight` / `dropRight` / `takeWhile` / `dropWhile` / `slice` / `splitAt` / `span` / `partition` / `reverse` / `distinct` / `init` / `last` / `headOption` / `lastOption` / `size` / `length` / `nonEmpty` / `contains` / `exists` / `forall` / `count` / `find` / `indexOf` / `mkString` (0/1/3 arguments) / `sum` / `product` / `min` / `max` / `reduce` / `reduceLeft` / `reduceRight` / `sorted` / `sortWith` / `zipWithIndex` / `grouped` / `sliding` / `toList` / `toArray` / `toSet` / `toVector` / `toSeq` / `Iterator.toList`. Anything not on `List` itself is a default method of `IterableOnceOps` / `IterableOps` / `SeqOps`, so it is called with invokeinterface, and return values erased to `Object` / `LinearSeq` are checkcast / unboxed. `scala.math.Numeric` (`IntIsIntegral` / `LongIsIntegral` / `DoubleIsFractional`) was added to the implicit scope for `sum` / `product`, and the `String` / `Long` / `Boolean` instances of `Ordering` for `sorted` / `max` / `sortBy`. On the **private runtime (`--no-scala-library`)** only what `crates/backend/src/runtime.rs` actually implements in the classfile is declared (`length` / `size` / `nonEmpty` / `last` / `reverse` / `filter` / `filterNot` / `contains` / `exists` / `forall` / `count` / `take` / `drop` / `mkString` with 0/1/3 arguments); everything else is **not silently accepted but reported as a diagnostic** (`value sorted is not a member of List[Int]`)
- Part of Predef: `assert` / `require` / `???` / ArrowAssoc's `->` / `identity` / `locally` / `implicitly` / `any2stringadd` (`1 + "x"`) / String's `length` and `toInt` (`toLong` / `toDouble` are there too). Under **`--scala-library`** these link to the jar's `Predef$` / `StringOps` / `Predef$ArrowAssoc` / `Predef$any2stringadd`. Also, only when linked against the jar: `intWrapper` / `RichInt` (`abs` / `max` / `to` / `until`), `longWrapper` / `RichLong`, `doubleWrapper` / `RichDouble`, `floatWrapper` / `RichFloat`, `charWrapper` / `RichChar`, `StringOps`'s `*` / `take` / `drop` / `isEmpty` / `toUpperCase` / `toLowerCase` / `stripPrefix` / `split`, the varargs `apply` of `Map` / `Vector` / `List` / `Set` / `Seq` / `LazyList`, `Either` (`Left` / `Right`), and `Try` / `Success` / `Failure` (`Try(1)` / `map` / `getOrElse`). This slice links **the conversion and aggregation members of ArrayOps** (`toList` / `toSeq` / `toIndexedSeq` / `toSet` / `toVector` / `toBuffer` / `groupBy` / `sortBy` / `sorted` / `sortWith` / `sum` / `product` / `min` / `max` / `minBy` / `maxBy` / `mkString` (0/1/3 arguments) / `reduce` / `reduceLeft` / `indexWhere` (1/2 arguments) / `lastIndexOf` / `patch` / `updated` / `appended` / `prepended` / `concat` / `++`. As confirmed with `javap -s scala.collection.ArrayOps`, `toList`/`toSet`/`toVector`/`toBuffer`/`sum`/`product`/`min`/`max`/`minBy`/`maxBy`/`mkString`/`reduce`/`reduceLeft` have neither an `$extension` nor a direct method on `ArrayOps` itself: at run time they wrap into a `scala.collection.mutable.ArraySeq` via `scala.Predef$.MODULE$.genericWrapArray` and then call the default methods of `scala.collection.IterableOnceOps`. `scala.math.Numeric` (`implicit object`s for `Int`/`Long`/`Double`) was newly added for `sum`/`product`/`min`/`max`/`minBy`/`maxBy`. The other methods just use the existing `Ordering`/`ClassTag` implicits) and **`scala.collection.MapView`** (`Map.view` / `keys` / `values` / `filterKeys` / `mapValues` (the type arguments can be inferred without being written) / `toMap` (the `A <:< (K, V)` witness `scala.$less$colon$less$.MODULE$.refl()` is synthesised on the codegen side) / `toList` / `toSeq` / `size` / `isEmpty` / `foreach`. No private MapView classfile is emitted) against the same jar
- Initialisation of traits with concrete `val`s (the interface's `static $init$`) and the `super` chain of `abstract override`
- `[B >: A]` widening in the collections: `Option.getOrElse` / `Option.orElse` / `immutable.Map.getOrElse` / `mutable.Map.getOrElse` have lower-bounded type parameters as in nsc, so the argument widens the result type up to the lub (`(o: Option[Sub]).getOrElse(base): Base`). It is the same mechanism as `List.::` (`prelude_lowbound.rs` / `prelude_ovl3.rs`). `scala.collection.mutable.HashSet` / `HashMap` / `LinkedHashSet` / `LinkedHashMap` are subtypes of `mutable.Set` / `mutable.Map` (and therefore of `scala.collection.Set` / `Map`). The view that lets an `Option` be used as an `IterableOnce` (`Option.option2Iterable`) is supplied from the jar's pickle (only under `--scala-library`; a diagnostic on the private runtime). Likewise `new StringBuilder(initCapacity: Int, initValue: String)` (also only under `--scala-library`; the private runtime's `StringBuilder` is `java.lang.StringBuilder`, which has no such constructor)
- Abstract type members and type projections: `trait Foo { type A; def x: A }`, `type A = Int`, and `Bar#A` in a method signature. A **type alias** on an object or class, `type T = List[Int]`, and a trait's `type A = String` are used as the underlying type in vals and defs. A cycle `type A = B; type B = A` is `illegal cyclic reference`. The pickle uses nsc's `ALIASsym` (2.13 has no `ALIAStpe` tag)
- Path-dependent types: the stable path `c.A` (with `c: Foo { type A = Int }`, or an object / `this` / a `val`). An unstable path such as a `var` or a `def` gives nsc's `stable identifier required, but … found`
- singleton / this-types: `x.type` on a stable path and `this.type` are typed and executed as result types. An unstable `x.type` (`var` / `def` / `new C()`) is reported as `stable identifier required`
- compound types: `A with B` is usable as the type of a value or a parameter, and members from both sides can be called. As a **type** it is accepted even with two classes (as in nsc; there simply is no value). Mixing a second class into a template (`class C extends A with B`) is reported as `class B needs to be a trait to be mixed in`
- Structural refinements: `{ def foo: Int }` / `T { def foo: Int }`. At run time this uses **Java reflection** (`getClass` / `Class.getMethod` / `Method.invoke` + unboxing). It is a subset of the same runtime semantics as 2.13's reflective calls. `scala.language.reflectiveCalls` is not required. **Structural assignment** `x.foo = v` (with `{ var foo: T }`, or a getter plus `foo_=`) and structural `x(i) = v` (`update`) are supported: reflective `foo_=` / `update`, as in nsc. The illegal `{ def foo: Int }; x.foo = 1` is `foo_= is not a member`. A `def` with a body is a diagnostic
- self types: typechecking and mixin of `trait T { self: Foo => ... }`. If the implementing class does not conform to the self type, `illegal inheritance`
- Variance: `class C[+A]` / `class Box[+A](val x: A)` are legal. `class Bad[+A](var x: A)` is rejected as covariant-in-contravariant, as in nsc. `A @uncheckedVariance` (in a method parameter or type argument position) turns off the variance check for that occurrence, as in nsc

- **Defining def macros**: `def f: T = macro Impl.method[A]`. This is parsed, the implementation reference is resolved, the
  `Impl$` / `method` binding is recorded on the symbol, and, as in nsc, **no bytecode is emitted** for the macro def
  (so it cannot be called from Java). An omitted result type, an implementation that is not a method on an object,
  an implementation that does not take a `Context` as its first argument, an unresolvable reference, and whitebox macros are all reported as diagnostics.
  The design is in [`docs/macros.md`](macros.md)
- **Expanding def macros (the JVM bridge)**: as in nsc, the macro implementation's classfile is
  **really loaded and called on the JVM**. Given `java` and scala-reflect.jar, a call to
  `def f(): Int = macro Impl.m` is expanded and the expanded program runs.
  The engine is a single Java file (`crates/typer/java/ScalaRsMacroEngine.java`);
  it is compiled with `javac` at the first expansion and cached in `$TMPDIR`, and one process is kept
  resident per compilation. The `Context` is built with a `Proxy`, and `universe` is filled with
  `scala.reflect.runtime.universe`. **As in nsc, the macro implementation must have been
  compiled in a previous run** (if it is in the same run, it is reported with the reason
  `is not on the macro classpath`). The argument shapes that can be passed
  (`Literal` / `Ident` / `Select` / `Apply` / `this`), the tags that can be built, and the kinds of trees that can be
  returned are a **subset**, and everything outside it is reported by name (it is never silently
  expanded into a different tree)
  ([`docs/macros.md`](macros.md) §7.11)
- **Implementations returning `c.Expr[T](tree)`, and `c.prefix`**: `scala.reflect.macros.Aliases` declares
  `Expr` twice, as a `val` (the extractor) and as a `def Expr[T: WeakTypeTag](tree: Tree)`.
  Explicit type arguments narrow the overload set **before** the value-position folding
  (the ordering of SLS 6.26.3), so `c.Expr[Int](tree)` resolves to the generated method.
  `c.prefix` passes the receiver tree at the call site to the engine, as
  `Expr[Nothing](tree)(TypeTag.Nothing)`, exactly as in nsc. A receiver that cannot be carried
  (`new`, or no receiver) is reported with a reason only when the implementation actually reads `prefix`.
  Along with this, a `WeakTypeTag[F[E]]` can now be synthesised from `appliedType` and the
  tags in scope (this holds outside macros too: `typeOf[List[Int]]` and
  `weakTypeOf[Option[Foo]]` are now accepted. However, for a constructor reached through a
  **type alias** such as `Predef.Map`, nsc keeps the alias while scala-rs names the class it points at,
  so only `toString` differs; `=:=` and `typeSymbol` agree), so macros
  **of the same shape as slick's `TableQueryMacroImpl.apply`**
  (returning `c.Expr[F[E]]`, taking a `WeakTypeTag[E]`, and writing `New(TypeTree(e.tpe))`)
  can be expanded. Program output matches in a dual run against real scalac 2.13.16
  (`tests/fixtures/ex_impl.scala` + `tests/fixtures/ex_use.scala`)
  ([`docs/macros.md`](macros.md) §7.12)
- **`Function` / `ValDef` in expansion results**: the
  `Function(List(ValDef(Modifiers(Flag.PARAM), TermName("tag"),
  Ident(typeOf[Tag].typeSymbol), EmptyTree)), …)` that slick's `TableQueryMacroImpl.apply` builds
  makes the round trip intact.
  `Modifiers` is carried **by flag name** (the values of `universe.Flag` are enumerated reflectively;
  nsc's bit layout is an internal detail, and moreover one bit can carry two names).
  Names not in the table, and any leftover bits with no name, are diagnostics rather than being silently dropped
  (because nobody would notice a `var` rebuilt as a `val`). Along with this,
  `import c.universe._` losing to the implicit `import scala._` was fixed
  (the priority ordering of SLS 2; `Function` was resolving to `scala.Function`),
  a `scala.Int` written as a path not becoming a primitive was fixed,
  tags for tuples, function types and arrays (`scala.TupleN` / `scala.FunctionN` /
  `scala.Array`) can now be built, and **applying the result of a macro that takes no arguments**
  (`M.f(1, 2)` where `f` takes none) reading the `Apply` as the macro's own argument clause was fixed
  (it now stops at the number of parameter clauses on the macro def).
  Program output matches in a dual run against real scalac 2.13.16
  (`tests/fixtures/sd_impl.scala` + `tests/fixtures/sd_use.scala`)
  ([`docs/macros.md`](macros.md) §7.13)
- **Reification of quasiquotes (`q"..."`)**: `q"..."` / `tq"..."` / `pq"..."` / `cq"..."` are not
  ordinary `StringContext` interpolators but **compiler-intrinsic macros** in nsc.
  The contents of the interpolated string are (after replacing `$x` / `${…}` / `..$xs` / `...$xss` with placeholders)
  **actually parsed by scala-rs's own parser**, and for `q"..."` they are desugared into calls to
  `<universe>.internal.reificationSupport.Syntactic*` and then typechecked and code-generated as ordinary
  expressions (`crates/typer/src/reify.rs`).
  The universe is taken from `import <universe>._`. What can be lowered is
  literals / names / selections / applications (including curried ones) / `$x` holes / `..$xs` holes, plus
  **`tq"..."` (type identifiers, selections, type applications, function types, tuple types, singleton types, type projections, compound types),
  `pq"..."` (`Bind` / extractors / `|` / `_: T` / stable identifiers), `cq"..."` (`CaseDef`),
  and, in `q"..."`, type ascriptions / eta-expansion (`f _`) / type application / blocks and `val` definitions /
  `new` / `match` / partial functions `{ case … }` / function literals / `this` / assignment /
  `if`-`else` / tuples**. Operator names are encoded with `NameTransformer`.
  **Definitions** can be lowered too (`crates/typer/src/reify_defs.rs`): `class` / `case class` /
  `trait` / `object` / `def` / `val` and `var` with modifiers (`SyntacticClassDef` /
  `SyntacticTraitDef` / `SyntacticObjectDef` / `SyntacticDefDef` /
  `SyntacticValDef` / `SyntacticVarDef`). The flags of `Modifiers` are translated into
  the bits of `scala.reflect.internal.Flags` (which are **a different thing** from the parser's numbering),
  and the parents nsc's parser supplies (`AnyRef`, plus `Product with Serializable` for a `case`),
  the class and parameter accessor flags (`PARAMACCESSOR` / `CASEACCESSOR` /
  `PRIVATE | LOCAL`), a trailing implicit clause (`ImplicitParams`), and
  the body of an anonymous class (`new C { … }`) are all reproduced.
  Every shape was read off real scalac 2.13.16's `-Ymacro-debug-lite`, and
  agreement with real scalac down to `showRaw` has been verified. **Any shape that cannot be lowered is
  always reported as `unimplemented syntax: quasiquote q"..." (which shape)`** (it is never silently accepted).
  What remains is the shapes the parser normalises away along with the distinction nsc keeps
  (an `if` with no `else`, by-name types), mixing `..$` with ordinary arguments, and `type` definitions
  ([`docs/macros.md`](macros.md) §7.4 / §7.7)
- **The 3 shapes that need fresh names**: the `_` placeholder function literal (`q"_.get"`),
  `_` type arguments, i.e. existentials (`tq"P[_, _]"`), and right-associative operators (`q"a :: b"`) expand in nsc
  not into a single expression but into a **block** that first places
  `val n = rs.freshTermName("x$")`
  (the name is drawn from the universe's counter at run time). scala-rs builds the same block.
  Infix `a :: b` and the dotted call `b.::(a)` become the same tree after parsing, so they are told apart
  by whether the text of the selection's span starts with the operator. A bare `_`
  type argument inside a pattern is a type variable pattern (`u.Bind(u.TypeName("_"), u.EmptyTree)`) and uses no
  fresh name, while a bounded one is an existential even inside a pattern
  ([`docs/macros.md`](macros.md) §7.10)
- **`Liftable` (holes that are not `Tree`s)**: a hole's argument need not be a `Tree`. nsc looks for an
  implicit `Liftable[T]` and splices in `Liftable.liftX[T](arg)`. scala-rs does no
  implicit search: it **picks the standard instance from the argument's type and directly builds the same
  tree that instance would build**. Literals such as `Int` / `String` become
  `u.Literal(u.Constant(v))`, a `Constant` becomes `u.Literal(c)`, a `Type` becomes
  `rs.mkTypeTree(t)`, a `WeakTypeTag` / `TypeTag` becomes `rs.mkTypeTree(tag.tpe)`,
  an `Expr[T]` becomes `e.tree`, a `Symbol` becomes `rs.mkRefTree(u.EmptyTree, sym)`,
  a `Name` becomes `SyntacticTermIdent` / `SyntacticTypeIdent` / `Bind` depending on the position it stands in,
  and `..$xs` becomes a per-element `xs.toList.map(v => …)`.
  To learn the type, the argument is speculatively typed before reification (the diagnostics are rolled back).
  **A type with no standard instance is reported by name**
  (`a hole of type X is not lifted (…)`). User-defined `Liftable`s are not searched for.
  The `q"($rModule.tupled) : ($uTag => $rTag)"` of slick's `ShapedValue.mapToImpl` is
  of this shape ([`docs/macros.md`](macros.md) §7.8)
- **`symbolOf[T]` / `weakTypeOf[T]` / `typeOf[T]` being found**: a member that writes its type parameters
  only in an implicit clause (the materialiser shape) was being dropped wholesale by
  `pin_undetermined_tparams`, giving `not found: value symbolOf`.
  It is now kept, but only in the shape where the clause is implicit-only and that implicit requires the very type parameter
  in question (like `classTag[Short]`, it is always called with explicit type arguments).
  Inside a macro implementation the `implicit rTag: c.WeakTypeTag[R]` fills the implicit, so
  `symbolOf[R]` really does resolve
- **Materialization of `TypeTag` / `WeakTypeTag`**: the implicit for `typeOf[T]` is
  written nowhere in the program. nsc does not say "not found": it expands the
  compiler-intrinsic macro `materializeTypeTag[T](u)` and **builds the tag on the spot**.
  scala-rs does the same at the same place (next to the `ClassTag` fallback in `fill_implicit_params_in`),
  assembling a block containing an anonymous `TypeCreator` class and typechecking it as an ordinary expression
  (`crates/typer/src/materialize.rs`). What it builds is
  `<universe>.TypeTag.apply[T](<universe>.rootMirror, new $typecreator1())`, with the
  creator's body being `$m$untyped.staticClass("Foo").asType.toTypeConstructor`.
  The universe is determined from the prefix of `import <universe>._` (read the same way as for quasiquotes).
  **The tree need not be identical to nsc's; what matters is that the runtime result of `tag.tpe` agrees**,
  which is checked in a dual run against real scalac 2.13.16 (`tests/fixtures/tt_tags.scala`, 30 lines matching).
  Because `staticClass` is a call naming exactly one class, what can be built is only
  **a top-level class type with no type arguments** (plus the primitives / `Unit` / `String` / `Any` /
  `AnyVal` / `Nothing` / `Null`); `List[Int]` / nested classes / `AnyRef` /
  type parameters / singleton types are refused with
  `materialisation is not implemented: cannot build a TypeTag for ...`,
  **naming the shape explicitly** (a different type is never built silently).
  Along the way, the missing symbol for `TypeTags$TypeTag$` (the classfile of an object nested in a trait
  carries no pickle of its own), the implicit parameter of `typeOf` being an
  unresolved `Type::Named`, and the `TypeTags#TypeTag` accessor not being supplied were all
  fixed. This is what makes slick's `c.typeOf[HList]` / `typeOf[Tag]` work
  ([`docs/macros.md`](macros.md) §7.10)
- **Expanding `reify { … }`**: `reify` is a compiler-intrinsic macro just like quasiquotes, and
  scala-reflect.jar has no implementation of it. scala-rs itself assembles
  `Expr.apply[T]($m, new $treecreator1())`
  (`crates/typer/src/reify_expand.rs`). **Hygiene** works as in nsc:
  a static `object` becomes `mkIdent($m.staticModule("..."))`, `.splice` becomes
  `x.in[$u.type]($m).tree`, and type arguments become `mkTypeTree` (a monomorphic class via
  `staticClass`, a type parameter via `tag.in($m).tpe` from the `WeakTypeTag` in scope).
  Locals, parameters, blocks, type ascriptions and types with no tag are
  **refused by name** (`cannot expand reify { ... }: ...`; a bare name is never assembled silently).
  Literals, applications to and selections on a static `object`, `.splice`, and type arguments all
  match real scalac 2.13.16 in a dual run (`tests/fixtures/rb_impl.scala` +
  `rb_use.scala`, [`docs/macros.md`](macros.md) §7.15).
  This is what makes slick's `reify { TableQuery.apply[E](cons.splice) }` in `TableQueryMacroImpl` work,
  taking `errors=115 → 113`
  (an `if` with no `else`, by-name types, by-name and varargs parameters,
  procedure syntax `def f() { … }`, pattern definitions, self types, early definitions) and
  `type` definitions
  ([`docs/macros.md`](macros.md) §7.4 / §7.7 / §7.8 / §7.10)
- **Refined `Context`, field enumeration through `MemberScope`, and mixed `..$`**:
  the first argument of a macro implementation may also be
  `blackbox.Context { type PrefixType = … }`
  (nsc's idiom for typing `c.prefix`).
  Enumerating the fields of a case class with
  `rTag.tpe.decls.collect { case s: TermSymbol => … }` now works (the pickle parents of
  `MemberScope` are read one level at a time, and member types are substituted by following the
  upper bounds of abstract type members). `..$xs` can be written **mixed** with ordinary elements —
  in an argument clause, a pattern argument clause, the statements of a block, or a template body — and
  lowers to the same `List(…) ++ xs ++ List(…)` as nsc's `reifyList`.
  Rank 2 (`...$xss`) is still refused by name. With this, slick's
  `lifted/ShapedValue.scala` goes from **5 to 0**, and `errors=99 → 94`
  (`tests/fixtures/sv_impl.scala` + `sv_use.scala`,
  [`docs/macros.md`](macros.md) §7.16)
- **Classes and traits read from `-cp`**: a Scala trait read from a classfile on the `-cp`
  is treated as an **interface** (`ACC_INTERFACE` is read), and
  **its parents are taken from the header's `super_class` / `interfaces`**. Previously the former was missing,
  giving `IncompatibleClassChangeError` at run time, and the latter was missing, making inherited members `is not a member`.
  Furthermore, when the JVM declaration of a member completed from the pickle lives in a class that is
  **unreachable along the bytecode path** (`scala.reflect.api.JavaUniverse`
  is an interface with `interfaces: 0`, while it is `scala.reflect.api.Constants` that declares `Constant()`),
  that internal name is recorded in `Symbol::declaring_class`, and
  codegen uses that class as the invoke's owner and `checkcast`s the receiver to it
  (the same shape as nsc). This is what makes Tree construction on `scala.reflect.runtime.universe` actually run
- **Members of a package object**: the `val`s and `def`s of a package object, such as the jar's
  `scala.math.Pi`. The typer folds these into the package symbol, but a package has no runtime value,
  so codegen pushes `<pkg>/package$.MODULE$` as the receiver
- **Inserting `apply` on the result of a no-argument `def`**: `mk("a")` for `def mk: Box` becomes
  `mk.apply("a")`. The extractors of the reflect API (`def Literal: LiteralExtractor` → `Literal(x)`) are
  of this shape. **It works across an overload set too**: the reflect API puts
  `val Ident: IdentExtractor` and `def Ident(name: String): Ident` side by side, so
  `Ident(TermName("x"))` matches neither candidate and becomes `Ident.apply(...)`
  (`Bind` / `This` / `New` are the same shape; slick's `TableQuery` macro implementation is
  written entirely with this)
- **Term selections not being eaten by a type member of the same name**: the reflect API puts down
  both `type Modifiers` and `def Modifiers(flags: FlagSet)`. Members of a jar are lazily loaded
  name by name, so when the type member went in first the term overload was never
  read and `u.Modifiers(flags)` resolved to `<notype>`
- **`import <value>._`**: when the prefix is neither an object nor a package but a **value**,
  the members of that value's *type* are brought in and unqualified references are rewritten to `value.member`
  (the `import c.universe._` shape). **It reaches the inherited members of jar classes too**:
  since those members are lazily loaded from the pickle name by name,
  `TermName` / `Literal` / `Constant` / `termNames` of
  `import scala.reflect.runtime.universe._` (all declared high up in the linearisation) were previously
  not coming in at all. **The type namespace is exposed separately as well**
  (the reflect API puts down both `val TermName` and `type TermName`).
  The prefix used for the rewrite is valid **only within that scope**
  (using a method-local `import u._` from another method produced a
  `getfield` on a dead local)
- **The signatures of macro implementations (`c.Expr[T]` / `c.Tree` / `c.WeakTypeTag[T]`)**:
  these are the type aliases `blackbox.Context` inherits from `scala.reflect.macros.Aliases`.
  Because the **type members** of jar classes can now be read from the pickle
  (`PickleSupply::complete_type_member`), the source of a macro implementation can be
  typechecked through scala-reflect.jar. They can also be reached from a refined receiver
  (`blackbox.Context { type PrefixType = … }`, the shape of slick's `mapToImpl`).
  Aliases are transparent, so `c.Tree` is `Trees.Tree` itself.
  **The prefix is dropped**, so the `Expr`s of two different `c`s are the same type here
  (in nsc they are different types. The erased signature that appears in the bytecode is the same
  `scala/reflect/api/Exprs$Expr` either way, so the output does not change).
  When scala-reflect.jar is not on the classpath, the `Context` stays empty and
  `value universe is not a member of Context` is reported
  (`--scala-library` does not include scala-reflect.jar)

For what actually works in the fixtures, see the table at the end of the README.
### Uncurry / Erasure

The pipeline is as follows.

```
parse → namer → typer → uncurry → lambda-lift → erasure → emit
```

As in nsc, uncurry is an independent pass between typer and erasure. It merges nested parameter lists into a single list and turns nested `Apply` nodes into a single call. Partial application and eta-expansion (`foo _`, an unapplied method in a position expecting a FunctionN) become `FunctionN` closures.

lambda-lift runs after uncurry and before erasure. It lifts nested `def`s in a method body into synthetic methods of the enclosing class, passing captured locals as leading parameters. This really does show up in the classfile and run, including when a nested def is eta-expanded as a value and when a lambda calls it recursively.

anon-capture runs immediately after lambda-lift and before erasure. For each class defined inside a method, it collects the free variables of the enclosing method in first-reference order and records them on the class symbol (`crates/typer/src/anon_capture.rs`). The backend uses that same ordering for the fields, the constructor parameters, and the arguments to `new`, so the two orderings always agree. What an inner class captures is also added to the outer class's captures, so even when nested, the values line up at the `new` site. A captured `var` is boxed into a `scala.runtime.*Ref`, and the box itself is what gets passed as an argument.

Erasure drops type arguments, turns type parameters and unbounded wildcards into `Object`, and inserts box / unbox at the boundary between primitives and `Object`. by-name is lowered to `Function0`. We do not rely on ad-hoc guesses in the backend alone. As in nsc, arrays are collapsed to `Object` **only when the element is an abstract type** (`def d[T](x: Array[T])` is `(Ljava/lang/Object;)`, while `Array[AnyRef]` / `Array[Any]` / `Array[AnyVal]` are `[Ljava/lang/Object;`).

`Unit` becomes `V` **only for a method return type**. In parameters, fields, array elements, and type arguments it erases to `scala/runtime/BoxedUnit` as in nsc, and the value is the `BoxedUnit.UNIT` singleton (`Nothing` likewise becomes `scala/runtime/Nothing$`). See "`Unit` arguments and `scala.runtime.BoxedUnit`" for details.

### Lambdas as `invokedynamic` (`agent/indy`)

A plain `FunctionN` literal is emitted as an **`invokedynamic`** rather than a closure class. This is the same shape as nsc 2.13's `-Ydelambdafy:method`.

```
val f: Int => Int = x => x + 1
```

```
// Main$.<init>
invokedynamic #48, 0   // apply:()Lscala/Function1;
putfield      Main$.f:Lscala/Function1;

// one extra method inside Main$ (no extra classfile)
public static final synthetic java.lang.Object $anonfun$0(java.lang.Object);
```

The classfile-side implementation lives in three places.

- `crates/backend/src/classfile.rs`: makes it possible to write `CONSTANT_MethodType` (JVMS 4.4.9) /
  `CONSTANT_MethodHandle` (4.4.8) / `CONSTANT_InvokeDynamic` (4.4.10) into the constant pool, and
  emits the `BootstrapMethods` attribute (4.7.23). The bootstrap table is owned by `Pool`, so
  entries accumulate into the same table across methods and identical contents fold into one entry.
- `crates/backend/src/code.rs`: `Assembler::invokedynamic_lambda`.
- `crates/backend/src/gen.rs`: `gen_function_indy` (the call site) and `emit_lambda_body`
  (the body method).

**The bootstrap is `LambdaMetafactory.metafactory`** (the 3-argument version), and
`samMethodType` and `instantiatedMethodType` are the same `(Object…)Object`. Because we write the body method
**in erased form** (both arguments and result are `java/lang/Object`, with primitive box / unbox inside the body),
there is nothing left for `LambdaMetafactory` to adapt, and no bridges are needed.

> nsc instead uses **`altMetafactory`**, passing `FLAG_SERIALIZABLE` (`1`), plus `FLAG_BRIDGES` (`4`)
> when `instantiatedMethodType` differs from `samMethodType`, and attaches a `$deserializeLambda$`
> whose bootstrap is `scala/runtime/LambdaDeserialize`.
> Furthermore, where primitive specialization exists, it points at a specialized interface such as
> `scala/runtime/java8/JFunction1$mcII$sp` and names the call site `apply$mcII$sp`
> (verified with `javap -c -p -v`). scala-rs does neither. We do not specialize because
> the call side also calls through `apply(Object)Object`, so it is consistent.
> We do not make lambdas serializable because **they were not `Serializable` back when they were synthetic classes
> either**, so this is not a regression, and because `LambdaDeserialize` is not in the private runtime.

**Where the body method goes.** At the point where the call site is being assembled, the enclosing `ClassBuilder`
is lent out to the `Assembler`, so methods cannot be added to it. The body is therefore pushed onto the
`Gen::lambda_bodies` queue, and once the class's methods have all been emitted, `Gen::drain_lambdas`
writes them out as static methods. So that nested class emission (anonymous classes)
does not mix them up, **each emitter drains only above the position (watermark) where it started pushing**.
If a body itself contains further lambdas they are pushed onto the same queue, so we keep draining until
the queue returns to the watermark.

**`$anonfun$N` is `public`.** A lambda inside a `PartialFunction`'s synthetic class has to point from that
closure class at a body in **a different class** (the real enclosing class). nsc's `$anonfun$` is
public static final synthetic for the same reason.

**The enclosing `this`.** In a synthetic class it was the `$outer` field; in a static method it is
**parameter 0**. `EmitCtx::outer_slot` holds that, and `load_this` emits `aload 0` instead of
`getfield $outer`. A non-local `return` (the key of `NonLocalReturnControl`) goes through the same path,
so it keeps working as is.

**What is still a synthetic class** (a deliberate fallback; mixing the two is fine):

| Shape | Reason | nsc does |
|---|---|---|
| `{ case … }` for a `PartialFunction` | the abstract methods are the two `isDefinedAt` / `applyOrElse`, so it is not a SAM | also a classfile |
| user-defined SAM types (`trait Transform { def run(s: String): String }`) | not supported yet | `invokedynamic` |
| 23 or more parameters | `scala.FunctionN` only goes up to 22 | the same |
| inside an interface classfile (the abstract side of a trait) | JVMS 4.6 forbids `ACC_FINAL` on interface methods; we emit no code here in the first place | — |

Which fallback was taken is printed to stderr with `SCALA_RS_LAMBDA_TRACE=1`
(`LAMBDA-FALLBACK partial-function` / `sam:<internal name>` / `arity` / `no-hoist-owner`).

**Effect** (184 slick files; measured alternately against the pre-change binary (rebuilt from `main`'s
`crates/backend/src`) on the same machine in the same time window):

| | Before | After | nsc |
|---|---|---|---|
| total classfiles | 4552 | **2127** (−53%) | 1498 |
| output size | 22 MB | **13 MB** | — |
| compile time | 215.6 s | 214.5 s | — |
| loading all classes (`Class.forName(initialize=false)`, min of 3 runs) | 267 ms | **155 ms** (−42%) | — |

Compile time is **essentially unchanged** (the difference is within noise). Instead of writing one closure
class's constant pool and three methods, we write one static method and one bootstrap entry, so the amount of
writing work barely goes down. What goes down is **output and load time**.

`errors=0 files_with_errors=0` and `verified=2127 failed=0` from `tests/slick_subset.sh` are unchanged.
Of the remaining 716 closure classes, **707 are `PartialFunction`s** and 9 are user-defined SAM types
(slick's sources contain 728 occurrences of `{ case`, so we are not emitting duplicates).

The fixtures are `indy1` (both ABIs) / `indy2` (byte-identical to the library and to real scalac) / `indy1_bad`,
and the test is `crates/cli/tests/indy.rs`.

### Method type-parameter inference (arguments plus expected type)

As in nsc's `instantiateExpecting`, a method's type parameters are solved with **both the arguments and the expected type** as constraints (`add_expected_constraints` in `crates/typer/src/check.rs`).

- In an **invariant position** of the result type, the expected type takes priority over the solution from the arguments. `Array` is invariant, so `val a: Array[AnyRef] = Array("x", "y")` gives `T = AnyRef` (`[Ljava.lang.Object;`), and `val b: Array[Any] = Array(1, 2)` gives `T = Any` with boxing.
- An expected type in a **covariant position** is only an upper bound, so the solution from the arguments wins (`cov("q"): List[Any]` gives `T = String`).
- Solved type arguments are fixed **before the implicit argument list is resolved**. Calling `def column[T](n: String)(implicit tt: TypedType[T]): Rep[T]` in a position expecting `Rep[Int]` goes looking for a `TypedType[Int]`.
- A type parameter that neither of these determines is not filled in with `Nothing`; we report the same diagnostic as nsc (`could not find implicit value …`).
- The expected type for an argument (its prototype) is passed down even for a callee that has
  **no type parameters at all**. This is limited to parameters that are "a function type, a `FunctionN`,
  or a SAM, and moreover fully determined" (`Typer::proto_arg_type` / `agreed_function_param`).
  When the argument **itself** is a function literal, `agreed_lambda_params` takes care of it, but the
  expected type was not reaching a literal that sits **inside** an argument, as in
  `f(if (c) { s => … } else { s => …; … })` (slick's `JdbcBackend`). The single-expression branches
  compiling was a coincidence: `section_param_types` merely happened to pick up the parameter types
  from the call in the body. For overloads (a case class companion's `apply` gives two candidates
  together with the inherited `AbstractFunctionN.apply`) we only do this when **all candidates** demand
  the same parameter type, and for constructors (`new C(…)` / `C(…)`) only when the class has no type
  parameters and the arity is unambiguous.
- An `Any` written as a type argument to a Java method is read as `Object`, the upper bound of that
  type parameter (nsc's `ObjectTpeJava`).
  `java.util.Arrays.copyOf[Any](a: Array[AnyRef], n)` (slick's `ConstArray`) is accepted even though
  `Array` is invariant, and the result is `Array[AnyRef]`. Passing an `Array[String]` is rejected,
  as in nsc.
- Arguments are converted to their **base type** in the parameter's class before being matched
  (nsc's `Types.baseType`; `align_to_param_class` / `base_type_instance` in `check.rs`).
  When `object OD extends D[Int]` is passed to `def u[A](d: D[A])`, the argument's type is `OD.type`,
  so `D[Int]` is only visible as a base type. The same holds for `this.type` / `p.type`: for a singleton
  type we first read **what it widens to** (`agent/hkinfer`).

Along with this, `Array` became **invariant** (`Array[Int]` cannot be passed where `Array[Any]` is expected, matching scalac). Also, the types of inherited members are now viewed through the **applied parent** (the implicit for `OptionMapper2[…, Boolean, …].column` looks for `TypedType[Boolean]`, not `TypedType[BR]`).

**Explicit type application** goes through the same path. An overloaded callee is first narrowed by
**the number of type parameters**, per SLS 6.26.3; if exactly one candidate remains we fix on it and only then
substitute the type arguments.
With `fs.typed[Boolean](ch)` (an overload of `def typed(tpe: Type, ch: Node*)` and `def typed[T : ScalaBaseType](ch: Node*)`),
without that narrowing an overloaded type stayed in `fun.ty`, and the subsequent implicit clause went looking for
an unsubstituted `ScalaBaseType[T]`.

#### Undetermined type variables (nsc's undetermined type variables)

Arguments are typed **without an expected type**, so that overload resolution is driven by types. As a result,
a polymorphic reference like `Map.empty` arrives in argument position still carrying its own type parameters
(`Map[K, V]`). nsc carries these around as **TypeVar**s (`Context.undetparams`) and solves them all at once after
the candidate has been chosen. scala-rs does the same (`undet_tvars` / `undetermined_of` / `undet_compatible` /
`instantiate_undet_arg` in `check.rs`).

- The applicability check (`arg_score`) unifies the variables an argument is carrying with the parameter type
  and solves them before comparing. `Map.empty` can be passed to `take(m: Map[String, Int])`.
  Empty `apply`s (`Map()` / `Vector()` / `List()`) go through the same path.
- Variables that an inner call could not solve are still undetermined for the outer call, so they are carried
  outward. In `take(id(Map.empty))`, the outer parameter type determines `K` / `V`.
- Variables that reach the result type are determined by the **expected type** (`solve_undet_result`).
  For `val l: List[Map[String, Int]] = f(Map.empty)` (with `def f[T](x: T): List[T]`), `K` / `V` are solved
  from the declared type. Varargs, by-name, and default-argument positions go through the same path.
- **Type parameters bound by an enclosing definition are fixed types, not variables.**
  We distinguish them by whether the name can be looked up in scope (`tparam_in_scope`).
  `def g[K](m: Map[K, Int]) = take(m)` and `def rec[T](x: T, m: Map[T, Int]) = take(m)` are rejected,
  as in scalac.
- The upper and lower bounds of variables are not ignored. A candidate whose unified solution does not
  satisfy the bounds is not chosen.
- Unsolvable variables are not silently filled in with `Nothing`; we report a diagnostic.

The reverse direction — the case where **the callee's own** type parameters are still undetermined — works the
same way. `xs.collect { case … }` is checked against `PartialFunction[Int, ?B]`, and `?B` is determined by the
literal's result type. Previously we collapsed the parameter type to `Any` here (`relax_open_tparams`), but that
was an ad-hoc hack that broke when carried into a place that loses the result type, so it was **removed** and
replaced by checking conformance against the solution derived from the argument (`solve_open_from_arg`).
As the expected type when typing arguments, undetermined variables are opened up to their **declared upper bound**
(`open_to_bounds`; `Any` if there is no upper bound).
Type parameters of a class that are in scope are fixed types, so they are not opened.
Calling `def take[T](r: Rep[T])` as `take(c)` (with `c: Rep[P1]`) inside `trait Base[P1]` gives `T = P1`;
it must not demand `Rep[Any]`.

Arguments to a parent constructor are matched **after substituting the parent's type arguments**.
For `class ReWrap[T : TT] extends Wrap[T](implicitly[TT[T]])`, `Wrap[A](val tt: TT[A])` demands
`TT[T]`, not `TT[A]`.

#### The expected type is also a **prototype** for the arguments (nsc's `protoTypeArgs`)

Even before a single argument has been typed, the expected type already pins down some of the callee's type parameters.
nsc's `Infer.protoTypeArgs` passes that down as the arguments' expected type (their prototype).
scala-rs does the same (`proto_arg_type` in `check.rs`).

```scala
def f(s: AnonSymbol, a2: Aggregate): (Node, Map[TermSymbol, Aggregate]) =
  (Select(...).infer(), Map(s -> a2))
```

`Map` is **invariant** in its key, so typing `Map(s -> a2)` without an expected type yields
`Map[AnonSymbol, Aggregate]`, which does not conform to `Map[TermSymbol, Aggregate]`.
With the prototype it becomes `Map[TermSymbol, Aggregate]`, as in nsc.

- This applies only to positions where **the parameter type is a type parameter as-is** (`Tuple2.apply[T1, T2]`).
  Widening it further would let the prototype start picking overload candidates.
- We do not do it when the callee is overloaded.
- The prototype is **a hint, not a constraint**. An argument that errors when typed with it
  (such as `kvs.toMap`, which still has an implicit clause) is rolled back along with its diagnostics
  and retyped without an expected type.

#### Empty varargs and `xs: _*`

A call that passes **nothing at all** to a vararg parameter provides no material for determining the element type.
nsc treats that as unconstrained and minimizes to the lower bound (`Nothing`). scala-rs was deciding that
"the arguments must solve it" merely by seeing that the type parameter "appears" in the callee's signature, so
`List()` / `Seq()` / `Map()` kept carrying the callee's type parameters and conformed to nothing.
We now look at the number of arguments actually passed and treat an empty vararg parameter as if it were not there.

An `xs: _*` argument arrives as a type whose element type is marked `Repeated`. The parameter side has already
been stripped down to the element by `param_at`, so **stripping only one side** made `def mk[A](xs: A*)` solve to
`A = Int*` and `mk(xs: _*)` become `List[Int*]`.
We now strip both sides (`unify_tparam_all` / `unify_one`). The `Map(kvs: _*)` /
`Seq(xs: _*)` / `Vector` / `Set` / `Array` factories all get the same treatment.

#### Dependent method types (nsc's `dependentTypeMap`)

```scala
def get[P <: Phase](p: P): Option[p.State]
```

`p.State` is the **argument's** `State`, not the abstract type member declared by `Phase`.
scala-rs's `Type::TypeMember` carries no prefix, so the result of `get(Phase.assignUniqueSymbols)` stayed
`Option[Phase#State]` and `.map(_.aggregate)` degraded to `Any`.

Since the prefix is not present in the type, we **look for the parameter that could have been the prefix among the
parameters themselves** (`subst_dependent_members` in `check.rs`). Only when **exactly one** parameter has the
abstract type member's owner among its base types do we replace it with the same-named member of that argument.
If it is abstract on the argument side too, nothing changes. The result is typechecked normally afterwards, so
`val bad: Option[String] = (new CS).get(new P1)` still gives
`type mismatch; found: Option[Int]  required: Option[String]`.

#### Higher-kinded application (`F[B]`)

When `F` is an abstract type constructor (`F[_]`), the result type of
`def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]` is a `Type::Applied`, not a `Type::Class`.
`collect_expected`, which solves type parameters from the expected type, was not looking at this shape, so
`B` was determined by neither the expected type `F[String]` nor the arguments and became `Any`
(every cats-style `F.flatMap(fa) { … }` came out as `F[Any]`). Two `Applied`s are now matched
constructor-to-constructor and argument-by-position. Type-constructor argument positions have no variance
annotations, so they are treated as **invariant** (i.e. a position where the expected type may override the
solution from the arguments). Where the expected type has already been reduced to a concrete class
(`F[B]` against `List[String]`), we match the constructor as the **unapplied** `List`, so that `F` itself
cannot be solved to `List[String]`.

### Implicit resolution

The search order follows nsc. There is no bogus "convert anything" rule.

1. The current scope, and the `implicit` members of enclosing classes / objects (including members inherited from a parent class / trait and members brought in with `import Foo._`)
2. The package object of the enclosing package (the implicit members of `package object p`)
3. The companions of the parts of the target type (its type constructor, type arguments, and nested prefixes) and of their **base classes** (`Option` for `Option[T]`, `Inner` for `Outer.Inner`, and for `A =:= B` the companion of `<:<`, which `=:=` inherits from). For a conversion, the parts of the source type are looked at too. When a companion exists only in a jar, we load its classfile just before the search and supply only the implicits from
   the pickle (**including `scala.*`** — the prelude only describes what programs write by name, and the witness of a class like
   `scala.collection.BuildFrom`, for which the prelude provides no companion,
   would appear in no scope at all unless the companion is brought in).
   Declarations from traits the companion mixes in are followed the same way
   (`object BuildFrom extends BuildFromLowPriority1 extends BuildFromLowPriority2`)

An implicit parameter clause can be given explicitly at the call site: `add(5)(3)` / `foo(x)(ev)`. The search only fills a clause in when it is omitted.

Numeric widening (`Int` → `Long` / `Double` and so on) is handled specially **before the implicit search**. It is built into the typer, not a scalac implicit.

Inherited implicit members are viewed **through the parent's type arguments** (as-seen-from).
When `trait Base[P1] { protected[this] implicit def p1Type: TT[P1] }` is used from
`trait Mid[P1] extends Base[P1]`, the candidate's type uses `Mid`'s `P1`, not `Base`'s `P1`
(`Typer::implicit_candidate_ty`). Leaving the raw declared type here makes
`implicitly[TT[P1]]` unable to find its own parent's implementation (slick's
`Library.Abs.column[P1](n)`).

A conversion's type arguments are solved after converting the receiver to its **base type** in the parameter's class
(`Typer::conv_targs` runs it through `base_type_instance`; `agent/mismatch14`).
Passing a `ConfigObject` (which `extends java.util.Map[String, ConfigValue]`) to
`implicit def mapAsScalaMapConverter[K, V](m: java.util.Map[K, V])` gave no type arguments at all to match on
the receiver itself, so both `K` and `V` degraded to `AnyRef` (`config.root.asScala` came out as
`Map[AnyRef, AnyRef]`).
The type parameters of an `implicit class` go through the same path
(`sub.firstOf: String` for `class Sub extends Base[String, Int]`).

Implicits brought in with `import <value>._` work the same way: they are viewed **through that value's type**
(`Typer::at_import_prefix_of`). Using `class Box[T] { implicit def mkOps(lhs: T): Ops[T] }` from
`b: Box[Int]` gives `Int => Ops[Int]`. Left as `Box`'s `T`, the candidate would match nothing.
The same prefix substitution applies when the result is a class **nested inside** that generic class
(the `T` in `def <(rhs: T)` of `Ordering[T]#OrderingOps` is `Ordering`'s parameter, not `OrderingOps`'s).
Furthermore, since this implicit is an **instance member**, the reference is name-qualified with that value as
the receiver. Emitting a bare name made codegen push `this` and cast, giving
`class Main$ cannot be cast to class NoTp`.
A conversion that a subclass **overrides** counts as one candidate
(`Integral[T]` narrows the result of `Numeric[T]#mkNumericOps` from `NumericOps` to
`IntegralOps`. Since the result classes differ, the "two paths to the same conversion" rule did not
drop one, and the search gave up).

Members of a jar are read **one name at a time**. But an implicit is something you
"find by searching a scope", so the program never writes its name, and neither
`Numeric#mkNumericOps` nor `Option.option2Iterable` was in any member listing
(slick's `import seq.integral._` and `where.reduceLeft(f)`).
For both `import <value>._` and "companions in a type's implicit scope", we ask the pickle
**which names are implicit** and complete only those names through the normal on-demand path
(`PickleSupply::implicit_member_names`). Names the class already has members for are not asked about, so
hand-written prelude declarations still win, as before.
Primitive companions are out of scope (`object Int`'s implicits are numeric widening itself, which the typer
has built in. Listing them as views would only make `n + ":"` ambiguous).

When there are several candidates of the same type, we decide by the same **sum** as nsc's `Infer#isStrictlyMoreSpecific`:
(difference in type specificity) + (difference in the subclass relation of the defining classes) > 0. Even for the same type,
the one defined in the more derived class wins (`ConstColumn[T : TypedType]`'s own evidence beats the `tpe`
inherited from `Rep.TypedRep`). When type and origin disagree, they cancel out and it is ambiguous — also as in nsc.

Failures are not stubbed out; we report a diagnostic.

- `no implicit: could not find implicit value of type …`
- `ambiguous implicit: …`
- `diverging implicit expansion for type … starting with method …`

#### Polymorphic implicit def / implicit val

When a candidate has type parameters of its own (`implicit def showList[A](implicit s: Show[A]): Show[List[A]]`),
we determine the type arguments by **two-sided unification** of the candidate's result type with the expected type
(`Unify` / `implicit_solve` in `crates/typer/src/implicits.rs`). Unlike the one-sided `unify_one`, this solves

- the candidate's type parameters (`A`), and
- the caller's **undetermined** type parameters (nsc's undetermined tparams; the `K` / `V` of
  `toMap[K, V](implicit ev: A <:< (K, V))` appear nowhere in the call, so only the search for the witness itself
  can determine them)

at the same time. The candidate side is widened to a base type when necessary before matching, so applying
`<:<.refl[A]: A =:= A` to `From <:< To` yields `A = From` and from there demands `From <: To`, which is the same
derivation as nsc (witnesses for `scala.<:<` / `scala.=:=` are not a dedicated fallback; `refl` is found by an
ordinary search for an `implicit`). The tuple sugar `(A, B)` and `Tuple2[A, B]` unify as the same type.

A candidate with type parameters left undetermined is **dropped** (we do not silently insert `Any`).
Once determined, the candidate's implicit arguments are resolved **recursively** (`Show[List[List[Int]]]` becomes
`showList[List[Int]](showList[Int](showInt))`).

Recursion has two cut-offs.

- A depth limit (`MAX_IMPLICIT_DEPTH = 8`)
- The equivalent of nsc's diverging implicit expansion: cut off when the same implicit is re-entered with the same
  head symbol for a target type whose complexity does not decrease (`implicit def loop[A](implicit a: A): A`).
  The diagnostic is `diverging implicit expansion for type Show[Int] starting with method loop`

Specificity follows nsc's `isAsSpecific`, comparing after collapsing the candidate's type parameters to wildcards.
`implicit val tagInt: Tag[Int]` is more specific than `implicit def tagAny[A]: Tag[A]`, so `tagInt` wins for `Tag[Int]`.
Two polymorphic implicits of the same shape are **ambiguous**.

When two implicits in a subtype relationship match the same target type (searching for `A` with both `A` and `B extends A` present), the more specific `B` wins. If the two types are the same, it is ambiguous as before. The origin (defining class) also works as in nsc: an implicit defined in a subclass has a more specific origin than one in the parent. If a more-specific implicit in the parent and a less-specific local in the child both match, type and origin disagree and it is **ambiguous**. The other way around (parent less specific, child more specific), the child wins.

An `implicit object X` counts as **one** candidate. Both the module symbol and the module class carry
`IMPLICIT` and have the same type, so as-is it became an `ambiguous implicit` with itself
(slick's `implicit object GetString extends GetResult[String]`). We drop the module class.

#### Retyping a call whose implicit arguments were already filled in

The typer sometimes types the same application twice (`retry_tupled_args`, which corresponds to nsc's
tuple adaptation, repacks the arguments into a single tuple and retypes the call).
The implicit arguments the first pass filled in are still in the argument list, so the second pass counted them
as "arguments the user wrote", turning `LiteralNode(1)` into
`not found: value intType` (it tried to look the companion's implicit up again in lexical scope) or
`no matching overload …(1, ScalaNumericType[Int])`.
Arguments the typer added itself are marked with `NodeId::FILLED_ARG` and dropped before re-resolution.

#### Residual implicit clauses in argument position

Passing `Array.empty` to `take(a: Array[String])` types the argument without an expected type, so it arrives
still as the method type `(ClassTag[T])Array[T]`. We show overload resolution its **result type**
`Array[T]` (treating `T` as one of nsc's undetermined type variables) and fill in the implicit clause only after
the parameter type has been determined. The witness we fill in is **the one the parameter type demands**, not
whatever single implicit happens to be in scope (with `take(empty)`, if only `Tag[Int]` is available the result is
`could not find implicit value of type Tag[String]`).

#### Type parameters only the implicit search can determine

The `T` in `def mk[T: TT](s: String): Seq[Int] => Rep[T]` appears in none of the value arguments, so only the
search for the witness itself can determine it (slick's `SimpleFunction.nullary`).
For a call whose second clause is entirely implicit, type parameters that could not be solved from the value
arguments are solved from the implicit parameter types, and that is reflected in the result type too.

#### Filling a function-typed implicit parameter (a view) from an implicit def

These are the views of SLS 7.2 / 6.26.5. An implicit parameter of type `A => B` can be filled, even without
a **value** of type `A => B`, by **the eta-expanded function value** of an **implicit conversion** from `A` to `B`.
Real scalac 2.13.16 passes
`$anonfun$main$1(int) = Predef.intWrapper(x)` /
`$anonfun$main$2(String) = Ordered.orderingToOrdered(x)(Ordering.String)`
to a call of `def h[A](x: A, y: A)(implicit ev: A => Ordered[A])`.

scala-rs had no such path. `fill_implicit_params_in` looked only for a **value** of type `A => B`, and failing that
tried just two hard-coded options, `identity_view` (`A <: B`) and `array_wrap_view`, so implicit defs were never
even candidates and, view bounds (`def f[A <% B]`, which desugars to the same implicit parameter) included,
you got `no implicit: could not find implicit value of type (Int) => Ordered[Int]`.

`Typer::conversion_view` in `crates/typer/src/views.rs` closes this gap. It is not a special case for `Ordered`.
It asks the ordinary view search `search_conversion(A, B)` and, if that finds something, merely builds
`(x$n: A) => x$n` and **has `adapt` adapt the body to `B`**.
This is exactly the same path and the same candidate selection as making `val b: B = (a: A)` compile, and it works
for any `A => B`.
The lambda is typed with a diagnostic mark, and if adapting the body reports anything we roll back and return `None`,
so we never accept something for which the search did not actually produce a witness
(passing `new Object` to `def h[A](x: A)(implicit ev: A => Ordered[A])` is rejected, matching real scalac's
`No implicit view available from Object => Ordered[Object]`).

Along with this, `search_conversion`'s candidate test (`conversion_provides` in `implicits.rs`) now looks at
**polymorphic implicit defs**. It used to compare declared types as-is, so a conversion with type parameters of its
own was entirely invisible to the view search, and `val b: Box[Int] = 3` did not compile even with
`implicit def boxit[T](x: T): Box[T]` present.
It now uses the same approach as the member-selection-side search (`conversion_result` / `conv_targs`):
solve the candidate's type parameters from the argument type, then compare result types. A conversion without a
witness for its own implicit clause is not applicable, as in nsc (otherwise `orderingToOrdered` would claim
`Box[Int] => Ordered[Box[Int]]` and then fail because no `Ordering[Box[Int]]` can be built).

#### Local-scope implicit conversions (views)

The `agent/localconv` slice. An asymmetry found by differential testing against real scalac:
the search for implicit parameters (`fill_implicit_params_in` → `Typer::implicits_in_scope`)
duly found `implicit val` / `implicit def` written in a method body / block / lambda body, but the
view search (`search_conversion` / `search_extension`) was not actually seeing the
"same candidate pool as implicit parameters" that SLS 7.3 talks about. Both call
`implicits_in_scope`, so they ought to walk the same scope chain; the root causes were
three, and none of them was the view search itself — all three were upstream of it.

1. **`Typer::type_def_sig` was not copying `Flags::IMPLICIT` onto local `def`s.**
   Members of a class / object are fine because the namer (`namer_member`) allocates the symbol with
   complete flags (`implicit` included) before `type_def_sig` runs, but a local `def` inside a block
   has no namer pass, and `type_def_sig` itself sees `tree.sym.is_none()` and allocates a fresh symbol
   with `Flags::EMPTY`.
   It then copied `LOCAL` / `PRIVATE` / `PROTECTED` but not `IMPLICIT`, so a local `implicit def`,
   although correctly entered into the block's scope, was completely invisible to every search
   (`implicits_in_scope` filters on `Flags::IMPLICIT`).
2. **The `implicit class` desugaring (`Typer::implicit_class_conversions`;
   `implicit class C(x: P) { … }` → a synthetic `implicit def C(x: P): C = new C(x)`)
   was only running for members of classes / modules.** The `TreeKind::Block` handling
   merely name-resolved local `class`es / `object`s and never called this desugaring at all, so for a
   local `implicit class` the conversion method itself did not exist — a problem before any search
   could even happen.
3. **`implicits_in_scope`'s scope search did no shadowing.**
   SLS 7.2's candidates are "identifiers referable without a prefix", i.e. subject to ordinary
   unqualified name resolution, which is supposed to **shadow**. But the implementation just walked
   every level of the scope stack and collected symbols carrying `Flags::IMPLICIT` without deduplication,
   so a local `implicit def i2s` with the same name as an outer `implicit def i2s` was not
   "shadowed, one visible" but "two candidates: `ambiguous implicit: i2s, i2s`".
   It now walks scopes from innermost to outermost and takes only names "seen for the first time in this
   scope", ignoring an already-taken name in the outer scopes (same-named instance members /
   package object members are shadowed the same way).

All three are fixed, and as a side effect one more independent bug that turned up is fixed too:

4. **The free-variable analysis in `crates/typer/src/lambda_lift.rs` was not propagating the captures a
   local class itself requires to the nested local `def` that `new`s that class.**
   The result of the `implicit class` desugaring (`new C(x)`) is always placed inside a synthetic
   method — that is, "another nested local `def`" — so making a class that captures a local, like
   `class C(...) { ... factor ... }`, into a local `implicit class` always hits this. It reproduces in
   the same shape for **plain** code with nothing to do with implicits, such as the
   `val factor = 10; class F(...) { def scaled = n * factor }; def helper() = new F(3).scaled`
   that scalac accepts, and threw
   `RuntimeException: cannot capture factor` at run time.
   `Symbol::captures` (which locals get captured) is computed by `mark_anon_captures`, but the driver
   runs that **after** `lambda_lift`, so at the point `lambda_lift`'s own free-variable analysis
   (`collect_captures`) saw `new F(x)` it was still empty. We now call `mark_anon_captures` once at the
   entry to `lambda_lift` (the driver's second call is harmless, merely recomputing over the lifted tree)
   to fill `Symbol::captures` first, and in `collect_captures`'s `New` branch we add the referenced
   class's captures (filtered by `own`) to our own captures.

The priority order is unchanged: local > import (same rank as local, since imports enter the scope) >
companion (`search_conversion` / `search_conversion_open` /
`view_undet_bindings` all consult `implicits_in_scope` first and fall back to companions when it is empty).

#### Do not silently eta-expand an implicit clause that was never filled

A method that takes only implicits is not a value. nsc either applies that clause or reports the missing
implicit; there is no third outcome. scala-rs had a third one. `adapt_implicit_apply` gives up in several
places (waiting for a `TypeApply`, or while typing an argument whose expected type is not yet known).
When nobody applied the clause afterwards, **the method type stayed as the expression's type** and `adapt`
**eta-expanded it into a function value**.
`println(List(Some(1), None, Some(3)).flatten)` compiled without errors and printed
`Main$$$anonfun$0@7a765367` at run time. That is a **silent miscompile**.
Written in a form where the type is visible (`List(Some(1)).flatten.sum`), the same tree surfaced as
`value sum is not a member of ((Some[Int]) => IterableOnce[B])List[B]`.

`Typer::reject_unapplied_implicit_clause` is the brake. `adapt` only runs when the tree is used **as a value**
under a known expected type, and `adapt_implicit_apply` has already tried once with that same expected type.
So a first clause that has survived this far will never be filled by anyone. We report it as a missing implicit
and do not eta-expand. Excluded are the case where the expected type is a method type (i.e. an enclosing `Apply`
is in the middle of applying it) and the case where the first clause has non-implicit parameters (i.e. it really
can be eta-expanded).

Along with this, when a function-typed implicit parameter carries the **caller's undetermined type parameters**,
those can now be solved from the view
(`view_undet_bindings` in `crates/typer/src/implicits.rs`).
The `B` in `flatten[B](implicit asIterable: A => IterableOnce[B])` appears nowhere in the call, so only the
witness can determine it, but that witness is a conversion, not a value. We unify the conversion's result type
with the expected type to solve `B`
(`Unify` widens the candidate side to base types, so `Iterable[Int]` can be matched against
`IterableOnce[B]`).

The companion of `scala.math.Ordered` and
`implicit def orderingToOrdered[T](x: T)(implicit ord: Ordering[T]): Ordered[T]` are declared in
`crates/typer/src/prelude_durrange.rs` (per `javap -p -s scala.math.Ordered$`, this is the only member of
`Ordered$`). It is `--scala-library`-only. The private runtime emits
`scala/math/Ordered` but neither `Ordered$` nor `Ordering`, so without the jar we still report
`no implicit: …` as before. The view path itself does not depend on the jar
(`tests/fixtures/dr_viewuser.scala` compiles under `--no-scala-library` too).

#### Gaps in the prelude

- The postfix units of `scala.concurrent.duration` (`5.seconds` / `100.millis` /
  `1.second + 500.millis`). These are `package object duration`'s
  `implicit def DurationInt(n: Int): DurationInt` (and `DurationLong` / `DurationDouble`), plus the
  20 unit methods of `DurationConversions` (`nanoseconds` / `nanos` / `nanosecond` /
  `nano`, four `micro` variants, four `milli` variants, `seconds` / `second`, `minutes` / `minute`,
  `hours` / `hour`, `days` / `day`). `Duration(5, SECONDS)` and `Duration.Inf` were already
  readable from the jar.
  These are value classes, so the conversion as seen by `javap` is `DurationInt(int)int` — an
  **erased identity** — and the classfile reader reads it as `Int => Int` with no `IMPLICIT`
  flag either (it is readable from the pickle, but `PickleSupply` excludes `scala/`).
  That is the whole of `value seconds is not a member of 5`. The unit methods really do exist as
  **ordinary instance methods** on the boxed `package$DurationInt` (the only `$extension`s are
  `durationIn` / `hashCode` / `equals`), so we read the box class from the classfile and add only
  the conversion. scalac lowers `5.seconds` to
  `new package$DurationInt(5).seconds()`, so the codegen for the conversion is
  `Intrinsic::NewWrapper` (`new <box>(argument)`).
  The package object is loaded lazily, so this introduction is done lazily too, from
  `Typer::package_object_of` (because `FiniteDuration`'s symbol does not exist until the jar is read).
  `crates/typer/src/prelude_durrange.rs`. `--scala-library`-only.
- `Range`'s companion `Range$`. The prelude declared only the class `Range`, so the identifier
  `Range` in term position resolved to the class symbol. `Range(0, 5)` found
  **that class's own** `apply(i: Int): Int` (the element accessor) and became
  `no matching overload for (Int)Int`. Per `javap -p -s
  scala.collection.immutable.Range$`, all `Range$` has is two `apply`s /
  two `inclusive`s / two `count`s (all `Int` versions); the `BigInt` / `Long` /
  `BigDecimal` versions live on the nested objects such as `Range.Long` (a separate slice).
  `apply` returns `Range$Exclusive` and `inclusive` returns `Range$Inclusive`, so we spell out the
  JVM descriptors (the same reason `RichInt.to` needed them in `gen.rs`).
  `--scala-library`-only (the prelude gates the class `Range` itself on `library_abi`, so without the
  jar even `1 until 10` is a diagnostic).
- **Supplying members from the pickle now happens before the view search**
  (`type_select`). The view of SLS 6.26.1 is inserted only "when the selection does not typecheck",
  and at that point nsc has finished reading all members. scala-rs reads members lazily, so putting
  the supply **after** the view search let "a member we simply had not read yet" lose to an implicit
  conversion. `1.second + 500.millis` is the example: `FiniteDuration`'s classfile spells `+` as
  `$plus`, so the member search missed it and `any2stringadd` hijacked the selection, giving
  `no matching overload for (String)String with arguments
  (FiniteDuration)`. Now the pickle supplies `+` and
  `FiniteDuration.$plus` is called. The condition "when nothing has been found" is unchanged, so this
  never hides an existing member.
- `scala.math.Numeric[T]` extends `scala.math.Ordering[T]` (the real ABI is
  `interface scala.math.Numeric<T> extends scala.math.Ordering<T>`). The prelude merely synthesized
  `Numeric` for `sum` / `product` without wiring up this parent, so a
  `Numeric[T]` could not be passed where an `Ordering[T]` was expected (slick's
  `ScalaNumericType[T] extends ScalaBaseType[T]()(tag, numeric)`).
  `crates/typer/src/prelude_numhier.rs`.
- **A method whose first argument list is implicit is not a view** (SLS 7.3: a view is
  "an *explicit* implicit method with one argument"). `implicit def Option[T](implicit
  ord: Ordering[T]): Ordering[Option[T]]` is a derivation rule, not a
  `Ordering[T] => Ordering[Option[T]]` conversion, yet the implicit-conversion search was picking it up
  without looking at whether the argument list was implicit. `val o: Ordering[Option[Int]] =
  Ordering.Int` **silently compiled**, and the receiver of a selection whose member was not found got
  rewritten by this conversion too (`value Int is not a member of
  Ordering[Option[AnyRef]]`), mangling the diagnostic. A method's **type** does not say which clause is
  implicit, so we decide from the parameter **symbol**'s
  `Flags::IMPLICIT` (`first_clause_is_implicit` in `crates/typer/src/implicits.rs`).
  Its use as a derivation rule (`List(Some(2), None).sorted`) keeps working.
- **Higher-kinded candidates.** When a candidate's type parameter is a **type constructor**
  (`buildFromIterableOps[CC[X] <: Iterable[X] with IterableOps[X, CC, _], A0, A]:
  BuildFrom[CC[A0], A, CC[A]]`), we match `CC[A0]` against `List[String]` to read
  `CC := List` / `A0 := String`, and answer `CC[A]` under the same bindings.
  This solves a `C` that **appears only in the implicit clause**, as in
  `LazyZip2.map[B, C](f)(implicit bf: BuildFrom[C1, B, C]): C`
  (see "[Higher-kinded implicit matching for `BuildFrom`](#higher-kinded-implicit-matching-for-buildfrom-lazyzip2-agentbuildfrom2)").
  Only **the candidate's own** type parameters may be assigned to a constructor; an undetermined
  `M[_]` on the caller's side is determined by ordinary inference from the arguments.
- **We check the bounds of the candidate's own type parameters** (nsc's `Infer#checkBounds`).
  `BuildFrom`'s witnesses have identical types apart from their bounds, so this is the only thing that
  distinguishes them. Higher-kinded bounds arrive folded into the type, so intersection-type unification
  handles those (`BuildFrom[CC[A0] with SortedSet[A0], …]`), and for a first-order F-bound
  (`buildFromBitSet[C <: BitSet with BitSetOps[C]]`) we look at `bound_hi`.
  All we check is whether the **class** the bound names is among the solution's base classes; we do not
  touch the argument positions (we only relax in the direction of being looser than nsc).
- The `apply` of a function value is the function itself. The prelude's `FunctionN.apply` is declared
  with erased parameters, so `f.apply(xs)` came out as `Any` (`f(xs)` was correct).
- The `copy$default$n` of a `case class` with varargs (`this.cells`) is typed as `Seq[T]`, not `T*`.
  nsc does not generate a `copy` for this shape, so checking against `T*` produced a diagnostic against a
  tree nobody wrote.
### Trait mixin

A trait with concrete members is emitted the way nsc 2.13 emits one, so that a subclass compiled by real scalac against our class files finds the implementations (`crates/cli/tests/traitclass.rs` compiles exactly that pair and runs it).

- The trait itself becomes a JVM interface
- A concrete body becomes a `default` method on that interface, with a
  `public static m$($this: T, ...)` beside it. The static is what every caller from outside the trait uses: `invokespecial` on a `default` method would require the interface to be a *direct* superinterface of the caller, which a trait several steps up the linearization is not
- A genuine `private` body is a `private static m$($this: T, ...)` with no declaration and no forwarder at all (JVMS 4.6 forbids `ACC_PRIVATE | ACC_ABSTRACT`, and nothing outside the trait may reach it) — see `crates/cli/tests/traitpriv.rs`
- `$init$` is a `static` method on the interface, emitted for **every** trait even when its body is empty: nsc calls it from every implementing class without checking
- A lambda in a trait body hoists onto the interface as a `public static $anonfun$`, again as nsc's do
- The implementing class emits forwarders — `invokestatic <Iface>.m$` — to whichever definition won linearization (the rightmost mixin is the more specific one)

With `class C extends A with B`, when A and B both have a `msg`, B is the one that runs. Linearization is Scala's C3 (`C extends Base with A with B` -> `C, B, A, Base`).

A trait's `val` is represented by a getter on the interface plus a **mixin setter `T$_setter_$v_$eq` with the same name nsc uses** (`p$q$T$_setter_$v_$eq` when a package is involved); `<Iface>.$init$` evaluates the right-hand side and calls that setter. The implementing class holds the field, and its constructor calls the mixin `$init$` methods (from the more general parent onwards). `object O extends T` works the same way, emitting exactly as many fields, accessors and `$init$` calls as a class does.

For `class D extends T { override val v = "d" }`, we make the **mixin setter an empty implementation (just `return`)**, as nsc does. `D` holds its own field and getter, and the constructor writes its own right-hand side after `$init$`, so the trait's initialization does not overwrite the override.

A trait's `var` gets a getter and an **ordinary setter `v_$eq`** (not a mixin setter), just as in nsc. For an abstract `var n: Int` as well, we emit `n()` and `n_$eq(I)` on the interface, and the implementing class's `var n` fills in both. Assignment from anywhere — the trait body, the implementing class, or outside (`d.n = 5`) — becomes an `invokeinterface` of `n_$eq` rather than a `putfield` to the field (a trait has no fields, so `putfield` would give `NoSuchFieldError`). Assignment to a trait's `val` is reported as `reassignment to val`, as in nsc.

For `abstract override` in a stackable trait, `super.m` inside the trait's body becomes `p$q$T$$super$m` — nsc's expanded name, after the trait's whole binary name (the implementing class forwards to the next entry in the linearization). With `class C extends Base with A with B` where both declare `abstract override def msg`, the runtime result is `B-A-base`.

#### A trait inheriting a class (SLS 5.3.3)

**A trait's parent may be a class**, as in `trait Loud extends Animal`. That parent is
a "constraint", not an initialization, so the trait **does not call** `Animal`'s constructor.
Consequently a trait's parent **cannot take an argument list** (`trait T extends C(x)` gives
`parents of traits may not have parameters`, the same as scalac 2.13.16), and no constructor
overload resolution happens at all. We fixed the case where a parent merely taking constructor
arguments produced `no matching overload for constructor`.

Because it is a constraint, only **subclasses of that superclass** may mix the trait in.
Mixing `Loud` into `class Plain` is rejected with the same wording scalac uses.

```
illegal inheritance; superclass Plain
 is not a subclass of the superclass Animal
 of the mixin trait Loud
```

At the classfile level, the trait's interface **does not extend that superclass** (scalac's
`Main$Loud` also has `java/lang/Object` as its super). So when a trait body reads an
inherited member through `this`, we emit a `checkcast` first (the JVM type of `this` is `LT;`,
so without it verification fails with `Type 'T' is not assignable to 'C'`).

Conversely, **when the class side has no class among its parents**, the trait's superclass
becomes that class's superclass (SLS 5.1). `class X extends Loud` extends `Main$Animal` in
the classfile too, and `val a: Animal = new X` verifies.

Since `abstract override` refers to **the next implementation in the linearization**,
`new Dog with Polite with Loud` and `new Dog with Loud with Polite` give different results
(`LOUD-please-woof` versus `please-LOUD-woof`).
When that chain does not reach a concrete implementation, we reject it **at compile time**
rather than failing at runtime.

```
object creation impossible.
abstract override def speak: String (defined in trait Loud) is marked `abstract` and `override`, but no concrete implementation could be found in a base class
```

The class's own definition sits **above** the traits in the linearization, so it cannot serve as
the target of a super call (as in scalac, this is `` `abstract override` modifiers required to override ``).
Putting `abstract override` on a class member is also rejected, as in scalac
(`` `abstract override` modifier only allowed for members of traits ``).

Linearization (C3) lives in exactly one place, `crates/typer/src/lin.rs`, and both typechecking
(deciding whether an `abstract override` is grounded) and code generation (super accessors and
mixin forwarders) use it.

#### Unresolvable parents are reported

If a name cannot be resolved at the head of `extends`, in any `with` term, in a type argument of
an applied parent, in a self-type annotation, or in `new X` / `new X {}`, we reject it with the
same wording as real scalac 2.13.16 (`not found: type X`, or `type X is not a member of package p`
when qualified). Previously we wrote a classfile extending `java/lang/Object` **silently, in both
modes**. See the section "Non-existent parent classes and traits were silently accepted
(`agent/parentcheck`)" below for details.

### The cake pattern across multiple compilation units (header pass)

`typecheck_units` typechecks the whole run against a single symbol table. There are four passes:
**namer (all units) -> header pass (all units) -> signature pass (all units) -> body pass**.

The header pass is `parents_pass` in `crates/typer/src/check.rs`. The namer records parents
**as names** (`rough_parents`), and `class_sym_of` resolves those names, looking them up in
**the scope current at that moment**. The signature pass walks units in command-line order, so
for a class whose parent chain lives in a later file, the grandparents were looked up in another
file's scope; the chain broke there and the entire inherited type became invisible. This is why
slick's `DB2Profile` (`slick/jdbc/`) referring to `Table`, an inner class of
`RelationalTableComponent` (`slick/relational/`) four levels up, produced `not found: type Table`.

```scala
// a.scala (first on the command line)
trait Child extends P1 { def f(t: Table[?]): String = t.n }
// z.scala (second)
trait TC { self: P1 => abstract class Table[T](val n: String) }
trait P1 extends TC
```

The header pass pins every unit's parent list to symbols **in the scope of its own definition
site** (including that file's imports). Because an inner class may name an outer inherited name
as a parent, we iterate until nothing changes (at most 3 rounds). Finally we make one more round
to attach **the primary constructor's parameter types**. `extends Table[Int](n)` is checked
against the parent's `<init>`, so when the parent was in a later file the argument types were
missing and we got `no matching overload for constructor`.

The header pass runs only for resolution, so **every diagnostic it produces is discarded** (the
real diagnostics come from the subsequent signature and body passes). Flags set by
`import scala.language.*` are saved and restored around it as well.

The self-type alias (the `self` in `trait T { self: P => }`) **is not inherited**. Everywhere that
brings parent or self-type members into scope consults `Symbol::self_alias` and excludes it.
Otherwise, in a cake like slick's where several components all call themselves `self`, `self`
would turn into an overload set.

The prefix of a type selection is a **term**, so for a companion pair the object side must be
chosen. When `trait Rep[T]` and `object Rep { abstract class TypedRep[T] }` sit side by side, the
`Rep` in `Rep.TypedRep` is the object (`qualified_type_owners` returns all candidates, and we take
the one that actually has that member).

### implicit and default arguments of a parent constructor

Even when `extends P` is written without arguments, if `P`'s constructor has an implicit clause or
a clause consisting only of default arguments, those arguments must still be passed on the JVM.

```scala
trait TT[T]
class TypedRep[T](implicit val tpe: TT[T])
class ConstColumn[T : TT] extends TypedRep[T]   // calls TypedRep.<init>(TT)
```

When the parent has **exactly one constructor of its own** and all the unwritten parameters are
either implicit or have defaults, `type_parent` rewrites the parent tree into the `extends P()`
form (an `Apply`) and then fills it in with the same `fill_defaults_and_implicits` used at call
sites. If it cannot be filled in, we do not silently accept it but report a diagnostic (scalac says
`could not find implicit value for parameter tpe: TT[String]`; we say
`no implicit: could not find implicit value of type TT[String]` at the same position). A `new P`
without arguments goes through the same rewrite.

The parent position is walked **three times — in the header pass, the signature pass and the body
pass** — so three things keep the second visit from breaking it.

- Filling happens **only in the body pass** (`sigs_only == false`). At signature-pass time the
  evidence parameters for a context bound on a parent in a later file may not exist yet.
- A filled tree is recorded in `parent_fill_done` (file / NodeId / span / class) so it is never
  filled twice. Same idea as `sig_done` / `lazy_done`.
- Synthesized arguments (`NodeId(0)` with a type attached) are **not retyped** on the next pass.
  Looking them up again by name would lose evidence parameters that are not in scope at that point.

Overload resolution uses **only the arguments written in the source**; filling comes afterwards.
Redoing resolution with the filled-in arguments would turn a single "implicit not found"
diagnostic into a `no matching overload for constructor`, and multiply the diagnostics.

Parent constructors also take **named arguments** (`agent/dbio`).

```scala
class MultiInsertAction(…)
  extends SimpleJdbcProfileAction[MultiInsertResult](
    _name = "MultiInsertAction",
    statements = rowsPerStatement match { … }
  )
```

Just as with `new C(b = 2, a = 1)`, we reorder them into parameter order **before choosing an
overload** (`reorder_named_ctor_args`). Without the reordering, `name = value` was typed as
"assignment to a non-existent variable", and this single place in slick produced three
diagnostics: `not found: value _name`, `not found: value statements`, and then
`no matching overload for constructor SimpleJdbcProfileAction with arguments (Unit, Unit)` from
the two remaining `Unit`s. When reordering fails (`unknown parameter name: …`) we return the tree
**without rewriting it**. The parent position is walked in the signature pass too, and diagnostics
from that pass are discarded, so consuming the named arguments there would leave only positional
arguments for the body pass, which could then only emit a different (and misleading)
`no matching overload`.

The scope of implicit search matches nsc as well. Parent constructor arguments are typed in the
constructor's own context, where `this` does not exist yet, so **the class itself and its
inherited members are not candidates** (we cut `implicits_in_scope` in
`crates/typer/src/implicits.rs` with `parent_ctor_scope`). Without this,
`class NullJdbcType extends DriverJdbcType[Null]` would use the very `implicit val classTag`
it is trying to inherit from the parent as the answer for the parent's `ClassTag[Null]`,
making it ambiguous.

Application of a parameterized type alias (`BaseColumnType[U]` against
`type BaseColumnType[T] = JdbcType[T] & BaseTypedType[T]`) is expanded in `is_sub_type`. The
evidence type created by the context bound `[U : BaseColumnType]` has this shape, so without the
expansion it does not conform to `JdbcType[U]` and the implicit is not found.

### try / catch / finally

We cover the `try` body with an exception table entry, and the handler tests the catch patterns
(`case _: RuntimeException` and so on) with `instanceof`. If nothing matches, we rethrow.
`finally` runs on both the success path and the catch path (we do not use `jsr`; we duplicate the
code).

What follows `catch` need not be a block of case clauses — it may also be a **value of type
`PartialFunction[Throwable, U]`** (`try close() catch ignoreFollowOnError`). We lower it to the
same tree as nsc's `makeCatchFromExpr`.

```scala
try close() catch ignoreFollowOnError
// ↓
try close() catch {
  case catchArg$1: Throwable =>
    val catchExpr$1 = ignoreFollowOnError
    if (catchExpr$1.isDefinedAt(catchArg$1)) catchExpr$1.apply(catchArg$1)
    else throw catchArg$1
}
```

The handler expression is evaluated **inside the case clause**, i.e. at most once, and only when
the body actually throws. An exception the handler does not accept is rethrown as is. A
`catch { expr }` that does not start with `case` is handled the same way, and `catch {}` remains
"no clauses".

When the `try` body always throws (`Nothing`), the type is the lub with the handler side, as in
nsc. `val n = try throw e catch toLen` is `Int`, not `Nothing`. We also take the lub when the
handler **does not conform** to the body's type: `try Success(f) catch { case NonFatal(e) => Failure(e) }`
is `Try`, not `Success`. This applies only when all branches are reference types; a shape mixing
`Int` and `Unit` keeps the body's type (the result goes into a single local, so mixing sorts would
require boxing).

That local carries its **declared type** (`Assembler::set_local_class`). When the branches put in
a `Success` and a `Failure`, the assembler's join — which has no class hierarchy — would come out
as `java/lang/Object`, and the following `areturn` would fail verification. A branch that puts a
primitive into a reference slot is boxed (`box_for_result_slot`; whether it is already boxed cannot
be told from the tree's type, so we look at the actual type on the assembler's stack). Joins for
`match` / `if` work the same way, using `Assembler::set_join_class` on the top of the stack. A join
with no declaration is `java/lang/Object`.

A **`return`** from the body or from a catch clause does not skip the finalizer. As in nsc, we
stash the value in a local, jump to a copy of the finalizer (placed **outside** the exception
table's range, so that a finalizer that throws does not run twice), and actually return there.
Nested `try ... finally` chains up from the innermost outwards. `synchronized { ... return x ... }`
goes through `monitorexit` by the same mechanism.

### Unreachable code

Instructions emitted by code generation after a `throw` / `return` / `goto` are **discarded** at
the next label (or at the end of the method). `def boom(): Int = throw e` ends with `athrow`, with
no `ireturn` after it (emitting one gives `VerifyError: Operand stack underflow`). In an
unreachable range we record neither stack map frames nor jump targets — a frame pointing at bytes
we are about to discard, and a label that joins an empty stack, both break verification. Since the
terminating instruction remains even when the method ends while unreachable, we do not get
`Control flow falls through code end`. Unreachable code **is still typechecked**
(`tests/fixtures/dead_bad.scala`).

An exception handler's frame names only the common supertype of the locals at the **entry** of the
covered range and the locals written within that range (the handler can be entered from anywhere in
the range).

### Nested types

`class Outer { class Inner }` becomes `Outer$Inner`, and a non-static inner class receives `$outer`
through its constructor. Overload selection between the primary and auxiliary constructors looks
only at the source arguments, but the `<init>` descriptor being called is prefixed with `$outer`.
`object Outer { object Inner }` becomes `Outer$Inner$` with a `MODULE$`.

An **`object` that is a member of a class or trait** is not a static singleton. The shape scalac
2.13.16 emits, as confirmed with `javap -v -p -c`, is the following, and we emit the same.

- `Main$Outer$P$` has an `$outer` field and a `public <init>(LMain$Outer;)V`
  (with a null check on the argument at the top). There is no `MODULE$` and no `<clinit>`.
  Only the field's visibility differs: instead of nsc's `private final` it is `public final`,
  matching an inner class's `$outer` (so that existing `$outer`-chain reads keep working).
- The enclosing `Main$Outer` gets a `private volatile Main$Outer$P$ P$module` and a
  `public Main$Outer$P$ P()` accessor that creates it under `synchronized` if it is `null`.
  The reference side calls `<outer instance>.P()` rather than `getstatic MODULE$`. That is why
  `o.P eq o.P` is `true` while comparing against another `Outer`'s `P` is `false`.
- When it is a trait member, the interface only gets a `public abstract <name>()`, and each
  implementing class emits the field and accessor (the same mixin treatment as a trait's
  `lazy val`).
- A **trait nested in a class** (`class Outer { trait T { def d = v } }`) cannot hold a field on
  the interface, so we declare the accessor `Main$Outer$T$$$outer()` abstract, using nsc's
  expansion name, and the implementing class or `object` implements it. The trait implementation
  (on the interface itself) calls that accessor rather than doing `getfield $outer`.

An **`object` inside a method body** (a local `object`) takes a different shape: nsc keeps one per
invocation in a `scala.runtime.LazyRef`, passing `$outer` and the captured locals to `<init>`. We
have not implemented that yet, so a local `object` that reads the enclosing instance or a local of
the enclosing method is **reported at compile time** (we do not silently emit a broken static
singleton). A local `object` that reads nothing from outside still compiles as a static singleton,
as before.

**Member classes of a trait** work the same way. As in nsc, the JVM type of `$outer` is the outer
trait's interface type (or `P`, if there is a self type `self: P =>` and it derives from the outer
trait), placed as the first parameter of `<init>`. Reading an outer `def` / `val` / `lazy val` /
type member from inside follows `$outer` and becomes an `invokeinterface`. Multi-level nesting
(`trait T { class Inner { class Deep } }`) follows `$outer` two levels.

The outer instance passed to `new` is determined in this order.

- **If a prefix is written**, as in `new p.Inner` (where `p` is a val / `this` / object), that one
- If it is reachable from `this` and its `$outer` chain, that one
- If not reachable, the enclosing `object` (`object O extends T { class R extends Inner }` passes
  `O$.MODULE$` to the parent constructor, as in nsc)

A class or object that extends a trait's member class passes `$outer` to the parent's `<init>` too.

**Declarations inside a method body** (local `trait` / `class` / `object`) emit just as much as
declarations inside a template. Binary names are indexed as in nsc (`Main$Same$1` / `Main$Same$2`),
and captures for a local trait go through accessors. See the section "Declarations inside a method
body (local trait / class / object)" for details.

### lazy val

**Members of a class, trait or object** get, in addition to the field, a `bitmap$0: Int` and a
synchronized accessor. Initialization is deferred until the first read.

A trait's `lazy val` (as in nsc's mixin phase) duplicates the field, the `bitmap$0` bit and the
accessor into each implementing class or object. Bits are numbered from a single list combining the
class's own `lazy val`s and the inherited ones, so they do not collide. The interface side has only
the abstract declaration, so calls are `invokeinterface`.

A **`lazy val` inside a method** (a local one) has no instance to hang a field on, so — as in nsc's
`lazyvals` phase — it becomes a **cell from the `scala.runtime.LazyRef` family**
(`crates/typer/src/lazy_local.rs`). At the declaration site we only create one cell; the initializer
runs at the **first read**, at most once, under the cell's monitor.

```scala
def f(n: Int) = {
  lazy val s = { println("mk"); "v" + n }   // only new LazyRef() here
  s + s                                     // calls s$1(s$lzy) on every read (initialized once)
}
```

- The cell type is determined by the result type: `Boolean`/`Byte`/`Char`/`Short`/`Int`/`Long`/
  `Float`/`Double` map to `LazyBoolean` ... `LazyDouble` respectively (without boxing the value),
  `Unit` maps to `LazyUnit` (flag only), and everything else to `LazyRef`.
- The accessor is handed to lambda lifting as an ordinary nested `def`, so locals, parameters and
  `var`s captured by the initializer are simply passed as extra arguments. Dependencies between
  `lazy val`s (`lazy val a = b + 1; lazy val b = 2`) also work as is, with `a`'s accessor capturing
  `b`'s cell (the same as scalac's `a$lzycompute$1(LazyInt, LazyInt)`).
- Inside a block, only a `lazy val` may be **forward-referenced** (a plain `val` is still an error).
- If the initializer throws, `_initialized` is set only after the value has been stored, so the cell
  stays uninitialized and the next read retries (the same as scalac).
- Declaring one in a loop body gives a separate cell per iteration.
- Under `--no-scala-library` we emit `scala/runtime/Lazy*` as part of the private runtime
  (`crates/backend/src/runtime.rs`). In jar mode we use the real ones and do not emit them.

Before this, the initializer of a local `lazy val` was **evaluated eagerly at the declaration site**.
Typechecking passed and the values were correct, so it was a miscompilation visible only in the
order `println` output appeared.

### Signatures of members without type annotations (lazy completer)

The type of `val p = 1` / `def p = 1` is not known until the right-hand side is typed. The typer
walks a template in source order, so a reference from a position before the definition would
naturally get `<notype>`. As in nsc, each symbol carries an "incomplete definition", and we complete
that definition **at the moment its type is needed** (`crates/typer/src/lazysig.rs`).

```scala
class C { def f: Int = D.p }   // D.p becomes Int
object D { val p = 1 }
```

The namer stashes the definition tree, and the typer's signature pass re-stashes it together with
the template's scope (imports, inherited members, type aliases). The completed tree is written back
into the original tree, so synthesis of evidence parameters and default getters never runs twice.

Re-entering a definition that is being completed produces the same diagnostic as nsc's
`CyclicReference` (we do not blow the stack).

```
recursive value y needs type          // object A { val x = y; val y = x }
recursive method f needs result type  // object A { def f = g; def g = f }
```

A `val` does not lock while its own right-hand side is being typed (a `def` does). This matches the
actual output of scalac 2.13.16: the two examples above agree down to the message, line and column.

`type T = rhs` rides on the same mechanism. Units are typed in command-line order, so if a signature
in an earlier file names `B.T` from a later file, the right-hand side is still unresolved and it
sees `<notype>`. A reference to a type alias completes the alias on the spot (on a cycle,
`illegal cyclic reference involving type T`). The same goes for an extractor's `unapply`: one
written without a result type, such as `def unapply(n: Nd) = Some((n.v, n.tag))`, is completed
before the pattern is typed, so the pattern does not see `<notype>` and count the sub-patterns as
one.

### Type aliases (alias type members)

`type Scope = Map[K, V]` is **the same type** as its right-hand side (nsc's dealias). We expand to
the right-hand side on either side of `<:`, as the receiver of `x.m`, and in erasure (`Scope` erases
to `Map`). Abstract type members (`type T <: Bound`) are not expanded and are still handled via
their upper bound.

When an override without a written result type inherits a type from the parent that contains an
**abstract type member**, we re-read it through the subclass's own concrete alias (nsc's
as-seen-from; `Typer::own_type_members`, `agent/mismatch14`).
If `trait Node { type Self <: Node; def rebuild(…): Self }` is implemented by
`case class StructNode(…) { type Self = StructNode }`, then `rebuild`'s result type is `StructNode`.

We also expand on the **path that solves a call's type parameters from the expected type**
(`collect_expected`). Given `object Type { type Scope = Map[TermSymbol, Type] }`, writing
`val s: Type.Scope = Map.empty` used to yield `Map[Nothing, Nothing]`, because without expansion
`Map[K, V]` and `Type$.Scope` do not line up structurally. Only aliases are expanded; abstract type
members are used as the expected type as they are.

The `p` in `p.T` in type position is a **term**. When a trait and a companion object share a name,
we look at the module class first (a class projection is written `C#T`). We keep the class as a
fallback for Java's static nested classes.

If the `A` in `new A(...)` is an alias, we **construct the right-hand side**, as in nsc
(`new Alias("hi")` for `type Alias = Base`). The alias symbol itself has no constructor, so without
this we got `no matching overload for constructor Alias`. The qualified form (`new p.A(...)`) was
already dealiased. Abstract type members (`type A <: Bound`) are out of scope (`new A` is not a
program, and constructing the upper bound is a different program).

#### Type aliases in a jar's package object

scalac writes nothing about a package object's `type`s into the classfile. They exist only in the
`ScalaSignature` pickle, so merely reading `<pkg>/package$.class` and folding in its members left
`scala.NoSuchElementException` (= `java.util.NoSuchElementException`) and
`cats.effect.Ref` / `Async` / `Resource` unresolvable.

**The first time a package object is needed**, we read the `ALIASsym` entries from its pickle and
register them as type members of the package (`SymKind::TypeMember`). We do not read ahead.

- For an alias with type parameters (`type Ref[F[_], A] = cats.effect.kernel.Ref[F, A]`), we
  reconstruct each type parameter's **kind (arity)** as well. Unless `F[_]` gets arity 1, the use
  site reports `does not take type parameters`.
- Classes named by the right-hand side are read from the classpath on demand. The pickle reader
  alone can only follow `scala.*`, so we have it report names it could not resolve, read the
  classfile on the typer side, and convert again — repeating that **until nothing new can be
  resolved**.
- The prelude and real classes always win. Aliases only fill gaps.
- **An alias whose right-hand side cannot be reconstructed is not registered.** Instead we remember
  the reason and, when that name is used, report something like `not found: type ParallelF -- package
  object cats.effect declares it as an alias for cats.effect.kernel.Par.ParallelF[F, A], which this
  compiler cannot express`. Saying what happened beats silently becoming `Any`.
- Alongside this we added an implicit `import scala._` (at higher priority than `import java.lang._`).
  It is a path consulted only when nothing else was found, so what actually arrives through it are
  the type aliases of the `scala` package object. Under `--no-scala-library` there is no pickle, so
  we supply nothing and report `not found: value NoSuchElementException`.

### super and qualified this

`super.m(...)` is `invokespecial` for a class parent, and `invokestatic <Iface>.m$($this, ...)` for a concrete
trait parent. The target of `super` is the rightmost parent in the linearization (we also parse and
honour the `super[T]` mixin qualifier). `Outer.this` follows an inner class's `$outer`.

**`super.T` in type position** works too. It is a path to a parent's type member, a spelling slick
uses heavily.

```scala
override def createUpsertBuilder(node: Insert): super.InsertBuilder = new SQLiteUpsertBuilder(node)
trait SimpleInsertActionComposer[U] extends super.InsertActionExtensionMethodsImpl[U]
```

We support it in return types, parameter types, the type of a local `val`, `extends` parents, and
the `C.super.T` / `super[Mix].T` spellings. A template's parent list is typed in **the outer
context**, as in nsc, so in `trait Mid` containing `class MidBuilder(m: Int) extends super.Builder(m)`,
the `super` is **Mid's**, not MidBuilder's.

`super` in a trait body (including `abstract override`) is the `T$$super$m` that the mixing class
fills in. A trait's `val` initialization is `$init$`.

### sealed and exhaustiveness

We record the `sealed` children (case class / case object / class) in the same compilation unit, and
if a `match` does not cover the leaves we emit a **warning**.

```
match may not be exhaustive. It would fail on the following input: …
```

As in scalac 2.13, this is not a hard error. Adding `-Xfatal-warnings` turns it into one. Guarded
cases do not count towards exhaustiveness. Wildcards and lowercase variables are catch-alls.

### unapply / unapplySeq

An extractor such as `Even(n)` calls `unapply` on the companion (or object). If it returns
`Option[T]` we use `isEmpty` / `get`; for `Boolean`, the truth value; for `Option[(A,B)]`, `Tuple2`'s
`_1` / `_2`. A pattern with no `unapply` gives `not found: extractor`.

`unapplySeq` covers the companions of `List` / `Seq` / `Vector` / `IndexedSeq` / `Array`, plus
user-defined varargs extractors. `List(a, b, c)`, `List(h, rest @ _*)`, `Seq(a, b)`,
`Vector(a, rest @ _*)`, `Array(a, b)` and `PairSeq(a, b)` all work. Named arguments are reordered in
a case class's constructor pattern (`Point(y = b, x = a)`).

`List` alone is traversed as a cons list via head / tail. Everything else is read by index, as in
real scalac (`lengthCompare$extension` / `apply$extension` / `drop$extension` on
`scala.collection.SeqFactory$UnapplySeqWrapper$`; `Array` uses the identically named extensions on
`scala.Array$UnapplySeqWrapper$`). This is why passing a `Vector` as a `Seq` does not fail. The type
attached to `rest @ _*` is the container of the extractor's own result type: `List[A]` for a `List`
pattern, and `Seq[A]` for `Seq` / `Array` patterns (the return type of `drop$extension`).

When the scrutinee's static type does not guarantee it is a sequence (e.g. `x: Any`), we emit a type
test first, as scalac does (`instanceof`; for `Array`, `ScalaRunTime.isArray(Object, 1)`). A
sub-pattern `_: T` is a **test**, so it fails via `instanceof` and we do not `checkcast`
(`case List((s, _: TableNode))` used to raise an exception on a value that did not match).

`SeqFactory$UnapplySeqWrapper$` does not exist in the private runtime (`--no-scala-library`), so
`case Seq(…)` / `case Array(…)` without a jar **produce a diagnostic** (we do not silently emit code
with element type `Any`). `List` patterns work in both modes.

### `x @ Pat` bindings and `null`

The `n` in `case n @ N(v, _)` is bound at **the pattern's own type** (`N`). We used to store it at
the scrutinee's type (`T`), so `n.copy(...)` went to read `N`'s fields off a value of the parent
type and gave `VerifyError: Bad type on operand stack`. The type-pattern spelling (`case n: N`)
worked, so it was only `@` that was broken. We emit it in the same order as nsc:
`instanceof` -> `checkcast` -> `astore`. If the test fails we do not bind either.
An `@` that narrows to a primitive, as in `case i @ (_: Int)`, unboxes before putting the reference
into an int slot.

`null` is treated exactly as in nsc for every kind of pattern (SLS 8.1.1 / 8.1.2).

| Pattern | Code emitted | `null` |
| --- | --- | --- |
| `case null` | `ifnonnull` (**reference comparison**) | matches |
| `case "a"` / `case 1` / `case 1L` | comparison with the constant on the **left** | does not match |
| `case Nil` (stable identifier) | same as above (`Nil$.MODULE$.equals(x)`) | does not match |
| `case s: String` / `case x: Any` | `instanceof` (emitted for `Any` / `AnyRef` / type parameters too) | does not match |
| `case N(v, _)` (case class) | `instanceof` | does not match |
| `case Ex(n)` (extractor) | fails first via `ifnull` | does not match |
| `case Seq(a, b)` | `instanceof` | does not match |
| `case _` | no test | matches |

We used to emit `case null` as `x.equals(null)`, which gave a `NullPointerException` on the one value
that case is supposed to catch. Emitting constant patterns in the `x.equals(constant)` direction was
fixed for the same reason (`case "a"` failed on a `null` scrutinee).

The constant-pattern comparison itself was also fixed. For `Long` / `Float` / `Double` scrutinees we
were popping both operands and **matching unconditionally** (`case 1L =>` hit every `Long`), so we
now use nsc's `lcmp` / `fcmpl` / `dcmpl` + `ifne`. A primitive constant against a reference scrutinee
is boxed before comparison (jar mode uses nsc's `BoxesRunTime.equals`; the private runtime uses
`Object.equals`).

`Null` conforms to no value type, so `(x: Int) match { case null => … }` is an **error**, as in nsc
(we do not silently emit a case that can never be taken).

```
type mismatch; found: Null(null)  required: Int
```

`case a: Array[Int]` does its `instanceof` with an array descriptor. Because `type_jvm_name`
returned `Object` for arrays, nothing was tested and `a.length` became an `arraylength` on an
`Object`.

The same story applies to the `==` operator. `x == null` / `null == x` (and `!=`) are, as in nsc, a
**single reference-test instruction** (`ifnonnull` / `ifnull`), with no `equals` call. The `null`
side is not evaluated (it is a literal, so there are no side effects). Value classes and primitives
cannot be `null`, so they do not take this shortcut and are boxed and compared as before.
A general `x == y` is nsc's `BoxesRunTime.equals` (null-safe) in jar mode, but the private runtime
has no `BoxesRunTime`, and a bare `recv.equals(arg)` failed when the receiver was `null`. Here we
follow nsc's own expansion.

```
if (recv == null) arg == null else recv.equals(arg)
```

Both sides are put into locals before branching, so the operand stack is empty at every branch target.

### Nested patterns (`case P(v) :: t`)

**An extracted value must not be `checkcast` to the sub-pattern's own type first.** nsc narrows the
extracted value to **the static type of where it came from** (for `$colon$colon.head` of a
`List[C]`, `checkcast C`), and only then emits the sub-pattern's `instanceof P` ->
`ifeq <next case>` -> `checkcast P`. scala-rs was unconditionally doing a `checkcast` to the
sub-pattern's type, so `case P(v) :: t` gave a `ClassCastException` for every list whose head was not
a `P` (typechecking passes, so you only find out when you run it). `case h :: t` on its own worked
because a sub-pattern that only binds is exactly the one that needs the narrowing.

The decision is centralized in `reads_erased_value` (`crates/backend/src/gen.rs`) and shared by every
path that bundles sub-patterns (case class constructor patterns, `unapply` results, `unapplySeq`
elements). Sub-patterns that **test** (`P(...)` / `Foo(...)` / `_: T` / constants / stable
identifiers / `x @ Pat`) receive the extracted value as it is, and only identifier patterns that
**only bind** get the narrowing. That fixed all of the following.

| Shape | Before the fix |
| --- | --- |
| `case P(v) :: t` / `case P(a) :: P(b) :: _` / `case h :: P(v) :: _` | `ClassCastException` |
| `case (p @ P(v)) :: t` | same (the inside of `@` was narrowed even though it is a test) |
| `case Some(P(v))` / `case Some(Nil)` | same |
| `case Some(1)` (`Option[Any]`) | unboxed to `Integer`, giving `ClassCastException`. We now keep it boxed and compare with `BoxesRunTime.equals`, as nsc does |

Two things were fixed at the `unapply` call site as well. A nested extractor receives an erased
`Object`, so we `checkcast` to the type the `unapply` descriptor requires. On top of that, when the
scrutinee's static type does not conform to the extractor's parameter type (`case Some(Two(a, b))`
against an `Option[Any]`), we put an `instanceof` -> `ifeq` in front and **fall through to the next
case**, as nsc does (previously it did not even verify). Also, because an `Option[(A, B)]` result was
left on the stack via `dup` when a sub-pattern jumped to the next case, a user-defined infix
extractor (`case P(v) ~ _`) gave `VerifyError: Inconsistent stackmap frames`. Tuples are now dropped
into a local before being read. An `unapply` returning a `Tuple3` or larger was also being
`checkcast scala/Tuple2` regardless of arity; it now uses `scala/TupleN`'s `_1()` ... `_n()`
(`Tuple2` alone keeps the `getfield`, since its fields are still public in 2.13).

Constructor-pattern arity is checked by the typer. Applying `case P(a, b)` to a one-field `P`
previously let `b` through as `Any`, and the backend threw
`RuntimeException("pattern arity")` at runtime. A varargs final parameter is out of scope.

### A `match` that falls through (`MatchError`)

A `match` that matches no case now throws a **`scala.MatchError` carrying the scrutinee**, as in nsc
(previously it was `RuntimeException("match error")`, which `case _: MatchError` did not catch and
which did not say which value it failed on). A primitive scrutinee is boxed before being passed. We
generate `scala/MatchError` into the private runtime as well (`crates/backend/src/runtime.rs`), so
both modes get the same class and the same message format (`<value> (of class <class name>)`, or
`null` for `null`).

### AnyVal (value classes and universal traits)

`class Meter(val n: Int) extends AnyVal` erases the value's representation to the underlying type
(`Int` here). `new Meter(x)` becomes `x`, and `m.n` becomes `m`. Methods become statics such as
`Meter.doubled$extension(n)`.

**Where a reference is required we box into a real `Meter` instance, as nsc does.** Not into an
`Integer`. Boxing is required for:

- assignment to `Any` / `AnyRef`, and parameters taking `Any` such as `println`'s
- a universal trait that `extends Any` (the `Univ` position in
  `final class Meters(val n: Int) extends AnyVal with Univ`). Boxing to `Integer` here gives a
  runtime `IncompatibleClassChangeError`
- type arguments (`List[Meters]` / `Option[Meters]` / arguments of a generic method) and array
  elements (`Array[Meters]` is `[LMeters;`). Lambda parameters too, since `FunctionN.apply` takes
  `Object`, so the boxed form is what arrives
- receivers of members the value class does not itself declare (`==` / `toString` / `hashCode`)

The reverse direction (unboxing) is `((Meters) x).n()`. The pattern `case x: Meters` is
`instanceof Meters` + `getfield`, `classOf[Meters]` is `Meters.class` (not `Integer.TYPE`), and
`x.asInstanceOf[Meters]` is `checkcast Meters`.

`equals` / `hashCode` are synthesized from the underlying value, as in nsc's `SyntheticMethods`
(we also emit the statics `equals$extension(u, that)` / `hashCode$extension(u)`). Without these, two
boxed `Meters(5)` would compare by reference, and `Object.toString` would print an identity hash
rather than `Meters@5`. `toString` for a case class whose field is a value class also prints
`Leg(Meters@3,b)` (the value is held unboxed, and only the printing boxes).

We box only **value classes that this compilation unit emits**. Library-side value classes held by
the prelude (`StringOps` / `ArrayOps` and so on) are not boxed, because `augmentString` is modelled
as an identity conversion and the underlying value is used directly as the representation
(`erasure::note_source_value_classes`).

A difference from nsc: nsc puts the body of an `$extension` static on the companion `Meters$` and
emits a forwarder on the class, whereas scala-rs emits it directly on the class. These are
equivalent within a single program, but they cannot interlink with a classfile emitted by scalac.

### Boxed types (`java.lang.Integer` and `scala.Int`)

As in scalac, these are **different types**. `scala.Int` is a value class and `java.lang.Integer` is
its box; travelling between the two goes through `Predef`'s 16 implicit conversions
(`int2Integer` / `Integer2int` / `char2Character` / `Character2char` / ...).

- `val i: java.lang.Integer = 3` (`int2Integer`) and `val n: Int = i` (`Integer2int`)
- Static members such as `java.lang.Integer.valueOf` / `parseInt` / `MAX_VALUE`,
  `java.lang.Character.isDigit`, `java.lang.Double.parseDouble`
- The boxes are ordinary reference types, so they can be written as type arguments: `add(7L)` on a
  `new java.util.ArrayList[java.lang.Long]`, `List[java.lang.Integer](1, 2, 3)`
- Numeric widening goes through, matching nsc's weak conformance: `xs.add(7)` (`Int` -> `Long` ->
  `long2Long`), `val i: java.lang.Integer = 'c'` (`Char` -> `Int`)
- The conversions are emitted as intrinsics (`Integer.valueOf` / `Integer.intValue`), so they work
  **in the private runtime** too, without requiring `scala/Predef$.int2Integer`

There were three fixes needed to keep them separate. (1) `scala.Int`'s `jvm_name` is its erasure
(`java/lang/Integer`), not an identity, so `classpath::find_by_jvm` skips primitive value classes
(`SymbolTable::is_primitive_value_class`). Without skipping them, `java.lang.Integer`'s classfile was
poured into `scala.Int` and `java.lang` ended up with no `Integer`. (2) For the same reason,
`add_package_paths` does not register value classes into `java.lang` either (`java.lang.Long` was
becoming `scala.Long`). (3) Java statics cannot be selected through an instance
(nsc: "Static Java members belong to companion objects in Scala"). Without this,
`java.lang.Integer.max(int,int)` competes with `RichInt.max` and breaks `1.max(2)`.
Note that `0.5.isNaN` resolves to `Predef.double2Double(0.5).isNaN()`, as in nsc
(`doubleWrapper` is on the `LowPriorityImplicits` side, so it has lower priority).

### The numeric widening tower and `Byte` / `Short`

nsc declares **all seven of `toByte` / `toShort` / `toChar` / `toInt` / `toLong` / `toFloat` /
`toDouble` on each of** `Byte` / `Short` / `Char` / `Int` / `Long` / `Float` / `Double`.
We add all 49 of them in `crates/typer/src/prelude_numconv.rs`, and code generation emits
`i2b` / `i2c` / `i2s` / `i2l` / `i2f` / `i2d` / `l2i` / `l2f` / `l2d` / `f2i` / `f2l` /
`f2d` / `d2i` / `d2l` / `d2f` from `Intrinsic::NumConv("<from><to>")` (a pair of JVM descriptor
characters) — see `gen::emit_num_conv`. `Byte` / `Short` / `Char` are `int` on the stack, so
converting to them means "widen to `int` width first, then `i2b` / `i2s` / `i2c`". All 7x7 have been
cross-checked against real scalac 2.13.16 in dual runs (including NaN, ±Inf, and each type's MIN and
MAX).

**`Byte` and `Short` are now genuine JVM primitives.** Previously the prelude gave them the JVM names
`scala/Byte` / `scala/Short`, classes that **do not exist**, so `def take(x: Byte): Int = x.toInt`
was emitted as `invokevirtual scala/Byte.toInt` and gave
`VerifyError: Type integer is not assignable to 'scala/Byte'`.
As with `Int` / `Long`, a value class's JVM name is its **box** (`java/lang/Byte` /
`java/lang/Short`). Along with that we fixed the following.

- Read Java's `byte` / `short` descriptors as `Byte` / `Short` rather than `Int`
  (`java.lang.Byte.valueOf(byte)`, the `Array[Byte]` of `String#getBytes`)
- Put `Byte` / `Short` / `Char` into the operator table both as receivers and as operands
  (`crates/typer/src/prelude_bsops.rs`). `b * 3` / `b < s` / `-b` / `~b` / `b << 2` promote to `Int`,
  as in nsc
- SLS 3.5.3 weak conformance `Byte <= Short <= Int <= Long <= Float <= Double` and `Char <= Int`
  (`val l: Long = b`). We added `Long -> Float` as well
- `Ordering[Byte]` / `Ordering[Short]` / `Numeric[Byte]` / `Numeric[Short]`
  (the jar's `Ordering$Byte$` / `Numeric$ShortIsIntegral$`; library ABI only)
- Allow `Int` constant patterns against `Byte` / `Short` / `Char` scrutinees
  (`case DatabaseMetaData.functionNoTable` is an `==` comparison, so nsc accepts it too)

**Element access on primitive arrays** was fixed in this slice too. `Array[Long]` /
`Array[Double]` / `Array[Char]` / `Array[Float]` / `Array[Byte]` / `Array[Short]` /
`Array[Boolean]` need dedicated instructions (`laload` / `dastore` / `caload` / `baload` ...)
rather than `aaload` / `aastore`. Previously everything except `Array[Int]` and `Array[Boolean]`
gave a `VerifyError` (and `Array[Boolean]` was using `iaload`, which is also wrong: on the JVM,
`boolean[]` uses the `byte` instructions).

While we were at it, we also fixed `Long.toInt` being emitted as `invokevirtual java/lang/Long.toInt`
(which does not exist), giving `NoSuchMethodError`, and `1 + 2.5f` pushing an `int` where a `float`
was expected, giving a `VerifyError`.
### Predef (this slice)

- `assert(cond)` / `require(cond)` (with the by-name message as the second argument). With the **private runtime** we `new` an `AssertionError` / `IllegalArgumentException` directly. With **`--scala-library`** we call `scala.Predef$.assert` / `require`
- `???` is `new scala.NotImplementedError` (a `RuntimeException` subclass) in the **private** mode. In **library** mode it is `Predef$.???` (the jar's `NotImplementedError` is an `Error`). Dual-run fixtures catch `Throwable`
- `1 -> "a"` via `any2ArrowAssoc`. In the **private** mode we `new` a `scala.Tuple2` directly (`Predef.ArrowAssoc` is never called). In **library** mode it is the implicit `any2ArrowAssoc` followed by `Predef$ArrowAssoc$.$minus$greater$extension`
- `identity` / `locally` / `implicitly`. In the **private** mode these are intrinsics. In **library** mode they are `Predef$.identity` / `locally` / `implicitly`
- `1 + "x"` via `any2stringadd`. In the **private** mode this is StringBuilder concatenation (an intrinsic). In **library** mode it is the implicit `any2stringadd` followed by `Predef$any2stringadd$.$plus$extension`
- `"x".length`. In the **private** mode this is `java.lang.String#length`. In **library** mode it is the implicit `augmentString` followed by `StringOps.size$extension` (the jar's StringOps inlines `length`, and the equivalent `size$extension` calls `String#length`). `toInt` / `toLong` / `toDouble` are `Integer.parseInt` and friends in the **private** mode; in **library** mode they are `StringOps.toInt$extension`

### Import resolution

The **prefix** of an `import` (the `a.b.c` part) is resolved **as symbols, one segment at a time**, rather than typed as an expression. A package that exists only inside a jar is not an expression (it has no type), so `import cats.syntax.all._` used to fail with `value all is not a member of <notype>`.

The prefixes that can be resolved are as follows.

- **Packages from the same run** (one level, two levels, three or more levels)
- **Packages from a jar**. We read `p/n.class` / `p/n$.class` on demand, and if there is none we create the **package itself** based on whether a `p/` prefix exists
- **Objects** and **package objects**. A package object is compiled to `p/package$`, and its members are members of `p` itself. Ones from the same run are folded in by the namer; ones from a jar are loaded and folded in here (the `all` in `import cats.syntax.all._` is `cats/syntax/package$all$`). **Type aliases** are not in the classfile, so we also read them from the pickle (see the "Type aliases in a jar's package object" section)
- **Term prefixes** (`import someObject.field._`) are still handed off to the typer as before

All four forms of selector work.

| Form | Example |
| --- | --- |
| Single | `import p.C` |
| Wildcard | `import p._` / `import p.*` |
| Named | `import p.{A, B}` |
| Rename / hide | `import p.{A => B}` / `import p.a as b` / `import p.{A => _, _}` |

A wildcard **enters the members known at that point first**, and also **records the owner in the scope**. That is because a jar package is read one class at a time without enumerating its entries, so right after `import cats.data._` the classfile for `NonEmptyList` has not necessarily been read yet. When an unresolved name shows up, the recorded owners are consulted in order (`Checker::expose_unqualified`). Names hidden by `{X => _, _}` are passed along this deferred path too, so hiding can never be broken after the fact.

A selector that resolved not a single name is **reported on the spot**, just like nsc (we do not turn it into an import that silently does nothing).

```
value Nope is not a member of package p1
```

A `case class C` has a **synthetic companion** before the `object C` later in the same file is named, so two Modules can answer to `C`. Prefix resolution returns **all candidates of the same kind** (the written object first) and looks the selector up across all of them. Only the best kind is kept, so `import scala.util.control.Breaks._` still points at the object rather than the trait of the same name.

The `scala.language` feature names (`existentials` / `higherKinds` / `reflectiveCalls` / `experimental.macros`) are also present as importable names (`crates/typer/src/prelude_lang.rs`). They gate nothing. Syntax that scala-rs cannot actually compile is still reported at the point of use, as before.

### Singleton types `X.type` and namespaces

Scala keeps **the term namespace and the type namespace separate**. The `X` in `X.type` is a term, so even when an inner scope binds `X` only as a type, we must still reach the outer term `X`. slick's `HList.scala` has exactly this shape.

```scala
object syntax {
  type HNil = hl.HNil.type      // HNil as a type
}
object HList {
  import syntax._               // brings the type name HNil inside
  def empty: HNil.type = HNil   // this HNil is the outer object HNil
}
object HNil extends HList { … }
```

We added `lookup_term` as the counterpart of `SymbolTable::lookup_type`, so that scopes which bind the name only as a type are skipped and the search continues outward (`is_stable_path` / `term_path_sym` / `term_path_type` / `type_ident` in term position).

In the same slice we closed two gaps around the prefix of `X.type`.

- **Package prefixes** (`p.HNil.type`). A package is not a value, so it has no type and `term_path_type` could not answer. We now go through `path_owner_sym`, which walks packages and modules directly. We also made `singleton_to_type` `expose_unqualified` the leading identifier (`p` may not be in scope yet).
- **Nested objects** (`ColumnOption.AutoInc.type`). The `I` in `object O { object I }` is a member of the module class `O$`, so when looking a member up from the prefix's type we normalize Module to module class (`path_member_owner`).

### The `Ordering` companion and summoning (`agent/ordsummon`)

`package object scala` exposes type classes unqualified **as both a type and a term**.

```scala
type Ordering[T] = scala.math.Ordering[T]
val  Ordering    = scala.math.Ordering
```

The prelude (`add_scala_aliases`) entered only the former, so `Ordering` in **term** position also resolved to the trait itself. As a result:

- `Ordering.Int` looked for a member of the trait and failed (fully qualifying it as `scala.math.Ordering.Int` did work).
- `Ordering[String]` **silently passed typechecking** as "a type application with a trait in term position", and codegen pushed `Ordering$.MODULE$` and checkcast it to `Ordering` (at runtime, `ClassCastException: scala.math.Ordering$ cannot be cast to scala.math.Ordering`). `Ordering[Int].reverse` shows up as an `IncompatibleClassChangeError`.

`crates/typer/src/prelude_ordsummon.rs` enters the companion modules into the term namespace as well (`Ordering` / `Numeric` / `Equiv` / `Fractional` / `Integral` / `BigInt` / `BigDecimal`). `SymbolTable::lookup` returns both the class and the module; term position (`type_ident`) picks the module and type position (`resolve_type_name`) picks the class. `Integral` / `Fractional` had no companion because `prelude_numhier` grew only the traits without reading the jar, so we create them here (the jar's `scala/math/Integral$` really does exist and has `apply:(Lscala/math/Integral;)Lscala/math/Integral;`). Before we created them, `val i: Integral[Int] = Integral[Int]` was silently accepted and then failed at runtime.

Summoning (`Ordering[String]` = `Ordering.apply[String]`; in nsc this is the identity `def apply[T](implicit ord: Ordering[T]): Ordering[T] = ord`) is handled by the `Module[T]` → `Module.apply[T]` redirect in `check.rs`. Two things were missing.

- The `apply` of a library companion is read from the pickle **when it is selected**, so it was never found for an `Ordering[String]` that does not write `.apply`. We now supply it from the pickle here. Putting this next to the collection factories, for which the prelude writes its own `apply`, is safe because `PickleSupply` now **refuses a copy of a hand-written member with the same erasure** (`agent/setapply`); before that gate went in, `List[Int](1, 2)` produced an `ambiguous overload`.
- The reference is not necessarily a module symbol. An alias in a package object arrives as an accessor (`def Equiv(): Equiv$`), so **stable values of module class type** get the same treatment (`module_class_of_value`).

Because the term `Ordering` is now a module, **one more overload-recovery path opens up**. Until now `BigDecimal(3L)` worked by the roundabout route "the term `BigDecimal` is a class → `apply` is not a member of that class → the `found.is_empty()` branch of `type_select` reads the companion's seven overloads from the pickle → `widen_with_companion` merges the two scopes". Once the alias resolves to a module, the module class has the prelude's three hand-written `apply` overloads, so `found` is not empty and the pickle is never read (`no matching overload for <(Int) | (String) | (BigDecimal)> with arguments (3L)`; this is the regression that got this slice reverted once). We added `widen_module_from_pickle` next to `widen_with_companion`, so that **even with a module receiver** the pickle is read only when "none of the candidates matched". It can only add (the `agent/setapply` gate refuses copies with the same erasure).

Under `--no-scala-library` there is neither a `scala/math/Ordering` classfile nor an `Ordering$`, so the diagnostic stays `not found: value Ordering` (`prelude_ordsummon` is gated on `library_abi`).

### Summoning `Equiv[T]` and `Ordering <: PartialOrdering <: Equiv` (`agent/eqtail`)

`implicitly[Equiv[Int]]` / `Equiv[Int]` are accepted by real scalac, but we failed with `could not find implicit value`. The real ABI (`javap -p -s`) is the hierarchy

```text
interface scala.math.Ordering<T>        extends java.util.Comparator<T>, scala.math.PartialOrdering<T>
interface scala.math.PartialOrdering<T> extends scala.math.Equiv<T>
interface scala.math.Equiv<T>           extends java.io.Serializable
```

but the prelude did not wire it up. There were two causes:

1. The edges `Ordering[T] <: PartialOrdering[T] <: Equiv[T]` were missing, so a widening assignment like `val e: Equiv[Int] = Ordering.Int` came out as a `type mismatch`.
2. `object Equiv` had no implicit instances at all. Real scalac picks an `Equiv`-specific instance (`Equiv$Int$`) for `implicitly[Equiv[Int]]`, not something derived via `Ordering.Int` (verified with `implicitly[Equiv[Int]].getClass.getName`).

`crates/typer/src/prelude_eqtail.rs` adds both. `Equiv` / `PartialOrdering` hit the same gap as the other `scala.math` type classes (`Ordering` / `Numeric` / `Integral` / `Fractional`), so we close it the same way: waiting for the jar's lazy load means being at a point where `find_by_jvm` cannot find anything yet (`install_prelude` runs before `install_classpath`), so at prelude time we build our own class plus companion module and `enter_in_current` them into the current scope. A later reference to `Equiv` / `PartialOrdering` from `check.rs`'s `expose_unqualified` sees "already in scope" and does not fire, so only these prelude symbols are used; members other than `equiv` (`fromComparator` / `by` / `TupleN` and so on) are supplied on demand by `pickle_supply` as long as `jvm_name` matches the real class (the same way `Ordering`'s `lt` / `gt` / `lteq` / `gteq` / `max` / `min` still work today).

Real scalac has no instance for `implicitly[PartialOrdering[Int]]` either, so adding the hierarchy edges must not make it summonable. We create no companion module for `PartialOrdering`, and hand-write only the implicit instances of `object Equiv` (`Unit` / `Boolean` / `Byte` / `Char` / `Short` / `Int` / `Long` / `BigInt` / `BigDecimal` / `String`, plus the deprecated `DeprecatedDoubleEquiv` / `DeprecatedFloatEquiv` for `Double` / `Float`, which became namespace objects in 2.13). Under `--no-scala-library` there is no `scala/math/Equiv` classfile and the diagnostic stays `not found: type Equiv` (`prelude_eqtail` is gated on `library_abi`).

#### The prelude type of `Ordering#compare` (same slice)

`add_ordering` in `crates/typer/src/prelude.rs` hand-wrote `Ordering[T]#compare` as `(Any, Any): Int`. That makes scala-rs alone silently accept calls like `Ordering[String].compare(1, 2)` that **real scalac properly rejects** (too permissive). `lt` / `gt` / `lteq` / `gteq` / `equiv` / `max` / `min` are not hand-written, and `pickle_supply` supplies the real ABI's `(T, T)` signature for them on demand, so only `compare` fell into this gap. The fix is simply to change the argument passed to `method()` from `Type::Any` to `Type::TypeParam(t)` (`Ordering`'s own type parameter). `Type::TypeParam` erases to `Ljava/lang/Object;` just like `Type::Any` (`jvm_desc` in `crates/backend/src/gen.rs`), so the erased descriptor `(Ljava/lang/Object;Ljava/lang/Object;)I` that the codegen for `sorted` / `sortBy` expects does not change. Only **how it looks** during typechecking changes.

#### Silently accepting `new T` / `new A` (a remaining item from `agent/parentcheck`, same slice)

These are the two forms that `agent/parentcheck` (the section above) left in Remaining.

```scala
def f[T] = new T   // scalac: class type required but T found
trait X { type A; def f = new A }   // scalac: class type required but X.this.A found
```

SLS 5.3.2 requires a class type for `new`, and neither a type parameter nor an abstract type member (one without `=`) is a class type. The `Ident` branch of `New { tpt }` in `check.rs` (the bare-name form with no type arguments and no qualification, as in `new T` / `new A`) used to try the "construct the right-hand side of the alias" transformation in `new_alias_target` and, if `found` (the result of name resolution) contained neither a `SymKind::Class` nor a type alias, pass it straight on to `type_expr`. Since a non-empty `found` also does not count as "not found", `new T` silently became a `new` expression wearing a `Type::TypeParam` and `new A` one wearing a `Type::TypeMember`.

The fix is a single step, run **after** `new_alias_target` returns `None` (meaning a jar-derived alias has already had dealiasing tried once), that looks for a symbol in `found` that is still a `SymKind::TypeParam` / `SymKind::TypeMember`. Because it looks only at **"resolved, and not a class"**, it is as careful as `agent/parentcheck`'s `strict_type_names` (which fires only when something is "genuinely not found" and lets legitimate jar-derived types that resolve later pass through), and it never mistakes a jar type alias that has not yet been read from the pickle for an abstract type member.

The message reproduces nsc's wording exactly. A type parameter is a bare name (`T`); an abstract type member is **`this`-qualified** (`X.this.A`) - nsc renders an unqualified type-member reference with an implicit `this.` prefix, and we match that too (`Typer::class_type_required_name`).

### When a newline ends a statement (nsc's `inLastOfStat` / `inFirstOfStat`)

`drop_non_separating_newlines` in `crates/lexer/src/lib.rs` follows the same rule as nsc's Scanners: it keeps a NEWLINE only when the token **before** the newline can end a statement, the token **after** it can begin one, and we are currently in a `{ … }` region (or at top level).

The parser has a matching rule. The loop in nsc's `postfixExpr` simply stops "if the current token is not an identifier", and since NEWLINE is a token in its own right, **the infix expression ends there**. That is,

```scala
val x = { 1 }
-1          // <- two statements, not `{1} - 1`
```

`}` is `inLastOfStat` and `-` is `inFirstOfStat`, so the newline survives and `-1` becomes a separate statement. The same holds right after `if (c) { 1 }`, right after `(1)`, and right after a line consisting of just an identifier.

When the operator is at the **end of the line**, the expression continues. This corresponds to nsc's `newLineOptWhenFollowing` (after pushing an operator, skip one NEWLINE if what follows can start an expression).

```scala
val a = 1 +
  -2        // <- one expression; evaluates to -1
```

Inside parentheses and brackets a newline never ends a statement in the first place, so a form starting with `(c` whose next line is `- 1)` is still subtraction, as before.

Previously we read "an operator after a newline continues the expression", so the first example above became `{1} - 1` and produced a diagnostic like `value - is not a member of Nothing`.

Newlines are not skipped unconditionally inside types either. Where `parse_compound_type` looks for `with` and the `{` of a refinement, it behaves like nsc's `newLineOptWhenFollowedBy` and skips **only when what follows the newline really is `with` / `{`**. Back when we skipped unconditionally,

```scala
trait A {
  val p: String
  println("x")     // <- was being swallowed as the infix type String println "x"
}
```

the statement on the line after a declaration with no right-hand side was swallowed into the type. See the "Expression statements in a template body" section for details.

Note that when an expression that has a value appears in **statement position** (`if (c) { buf += x }` or `x match { case … => 1 }`), we generate it with `expectedType = UNIT`, just like nsc's `genLoadIf` / `genLoadMatch`. Generating a branch where only one side leaves a value at the lub type `Any` left the stack heights unequal at the join point, producing `VerifyError: Inconsistent stackmap frames`.

### Function literals in block position (nsc's `expr(InBlock)`)

nsc's `expr(location)` reads the body of a function literal that appears **as a statement of a block** with `block()` rather than `expr()`. That is, `{ x => val n = 1; n }` is not "a `val` written in expression position" but **a lambda whose body is a block**. We now do the same. `parse_block_stat` sets `in_block`, `parse_expr1` consumes it (nested subexpressions revert to nsc's `Local`), and the body after `=>` is read with `parse_case_body` (up to `case` / `}` / EOF).

We also added nsc's `typeOrInfixType(location)`. A type ascription in block position stops at `InfixType`, so the `=>` in `{ x: Int => body }` **belongs to the lambda, not to a function type** (in `Local` position, for example inside parentheses as in `(f: Int => Int)`, it is still a function type). While we were at it we reordered things to match nsc (type ascription → `=>` → `match`).

This is plain 2.13 behavior, not `-Xsource:3`. slick uses the shape `state.map { tree => val replace = …; … }` a lot, and this single point made 17 files fail to parse right at the top.

### `?` wildcard types and `-Xsource:3` `&` intersection types

**`?` wildcard types** (`List[?]` / `Shape[? <: Level, T, ?]` / `? >: Lo <: Hi`) are an alias for `_` and lower to exactly the same anonymous `TypeDef` as `_`. scalac 2.13.16 **accepts `?` even without `-Xsource:3`** (`?` is reserved, and using it as a type name requires backticks), so we accept it without the flag too. Matching real scalac, we report a diagnostic for un-backticked `type ?[A, B]` and `Int ? String`.

```
using `?` as a type name requires backticks
```

A backticked `` `?` `` remains an ordinary name. While we were at it we also made `_ >: Lo <: Hi` (lower bound then upper bound, nsc's spelling) work. Previously we looked only at the upper bound and produced `expected ], found subtype`.

**`&` intersection types** (`R <: Product & Serializable`) are accepted only under `-Xsource:3` / `-Xsource:3-cross`, and lower to **the same tree** (`CompoundTypeTree`) as a 2.13 compound type built with `with`. This holds when mixed with `with` as in `A & B with C { def f: Int }`, and with a refinement attached. Without the flag we stay plain 2.13, where `&` is treated as an ordinary infix type constructor and reported (scalac says `not found: type &`).

**Vararg patterns `case Cast(ch*)`** are likewise accepted only under `-Xsource:3` / `-Xsource:3-cross`, and lower to **the same tree** (`Bind` + `Star`) as 2.13's `case Cast(ch @ _*)`. As in nsc, the case of the name does not matter (`Foo(One*)` is a binding `One @ _*`, not matching against the stable id `One`), and only a `*` immediately before `)` counts, because `case p * q` is an infix extractor at every source level. Without the flag we report the same wording at the same column as scalac 2.13.16.

```
bad simple pattern: use _* to match a sequence
```

This is the slice for getting slick (whose `build.sbt` passes `-Xsource:3` / `-Xsource:3-cross`) through. `?` is used in 59 of slick's 176 files, `&` in 41 places, and `ch*` in 2 places.

**The vararg splat `f(xs*)`** is also accepted only under `-Xsource:3` / `-Xsource:3-cross`, and lowers to **the same tree** (`Typed` + `<repeated>[_]`) as 2.13's `f(xs: _*)`. The infix loop gives up on the right-hand side before `)` and makes a postfix `Select` `xs.*`, so we read only the `*` that closes an argument list as a splat. Since varargs can only be the last argument, there is no splat anywhere else. Without the flag we stay plain 2.13 with a postfix operator, and produce the same wording as scalac 2.13.16.

```
value * is not a member of List[Int]
```

slick uses it in 3 places, such as `Map(elems*)`.

**The `*` wildcard import and `as` renaming** are also accepted only under `-Xsource:3` / `-Xsource:3-cross`, and lower to **the same trees** as 2.13's `_` / `=>` (`import p.*` becomes `import p._`, and both `import p.{a as b}` and `import p.a as b` become `import p.{a => b}`). Without the flag `*` stays an ordinary name, so like scalac 2.13.16 we report at the import selector position (scalac says `object * is not a member of package p1`; we say `value * is not a member of package p1`). slick uses it in over 60 places, such as `import slick.ast.*`. `given` / `using` are not 2.13 syntax and are therefore **out of scope**.

### Type members that take type parameters, and higher-kinded context bounds

This slice fixes, against real scalac 2.13.16, the shapes that slick's profile cake uses heavily. Minimal reproductions live in `tests/fixtures/tmember{1,2,3}.scala`, and `crates/cli/tests/tmember.rs` **compiles them with both scalac and scala-rs and compares the output of `Main`**.

**1. Override checking for type members with type parameters**. When `trait A { type C[T] <: TypedType[T] }` is implemented by `trait B extends A { type C[T] = JdbcType[T] }`, the parent's `T` and the child's `T` are **different symbols**. Previously we compared `JdbcType[T_child]` against `TypedType[T_parent]`, which always failed and produced `incompatible type in overriding`. As in nsc, we now **substitute the parent's type parameters with the child's** before comparing. Furthermore, when the bound refers to a sibling member (`type B[T] <: C[T]`), we re-read it according to how the child implemented that `C` (`expand_type_members`). An actual bound violation (`<: Bound[T]` against `type C[T] = Int`) is still reported as before.

**2. Context bounds on higher-kinded type parameters, `F[_]: Async`**. After confirming that real scalac 2.13.16 **accepts** both `def f[F[_]: Async]()` and `class C[F[_]: Async]`, we now desugar them to `(implicit ev: Async[F])`. The README said "HK context bounds give `takes type parameters`, as in nsc", but **that is wrong**: what is rejected is only the *view* bound `F[_] <% V` (which we still report as before). We also apply the context bound when its bound is a **type member that takes type parameters** (`def base[U: BaseColumnType]` becomes `BaseColumnType[U]`).

**3. Separating the type and term namespaces**. With something like `trait D[F[_]] { def g = { val F = asyncF; val u: F[Unit] = … } }`, where a `val F` shares its name with the type parameter `F`, even the `F` in type position was swallowed by the term and we got `not found: type F`. Type-name resolution now uses `lookup_type`, which **looks only at the type namespace and escapes to the outer scope**.

**4. Resolving type members out to the enclosing instance**. When going through an **inner class** of the trait that declared an abstract member, as in `Main.factory: Main.Factory`, nsc reaches the implementation via the outer instance's prefix. `expand_type_members` now looks as far out as `from`'s **lexically enclosing class**. We also re-read the result of the application from `this_class` when the alias body of a type member still names an abstract member (`type C[T] = self.C[T]`).

**5. Diagnostics for unresolved type names**. `Missing[Int]` is now **`not found: type Missing`** (nsc's wording) rather than a kind error. Previously the wording was the misleading `Missing does not take type parameters`.

**6. Member references through a type member's upper bound**. So that `c.name` can be looked up on a `c: C[U]` where `type C[T] <: TypedType[T]`, `class_sym_of` for abstract members with type parameters now resolves **by following the upper bound**. A visited set stops it from looping on mutually recursive bounds (`type Self >: this.type <: Self`). While we were at it we fixed `subst_as_seen_from` infinitely recursing on `Applied { ctor: TypeMember }` (because `apply_type_ctor` returned the same type).

**Wildcards inside bounds** such as `Rep[?]`, **passing a higher-kinded parameter as a type argument** such as `Query[?, U, C]`, **`#` projections with type arguments** such as `Profile#AbstractTable[?]`, and **aliases with type parameters in a package object** (`type DBIO[+R] = DBIOAction[R, NoStream, Effect.All]`) were all confirmed with minimal reproductions to have worked already before this slice.

Measurement (`tests/slick_measure.sh`, 177 slick files, `-Xsource:3`) shows typecheck errors going from **13,245 to 13,164**, of which **kind-related errors went from 605 to 34**. Many of the "`X does not take type parameters`" cases were in fact **unresolved type names**, and now correctly say `not found: type X`.

**Remaining**:

- **Inference for higher-kinded type parameters** (passing an anonymous subclass to `def take[U, C[_]](q: Query[?, U, C])` cannot fit `C`; explicit type arguments work).
- The remaining 34 kind errors are almost all downstream of **other work**. `ColumnOption` / `::` / `Ordering` are cases where the **star-form wildcard import** of `import slick.ast.*` is not taking effect (`import slick.ast._` works), so the same-named prelude entries (`scala.math.Ordering` and so on) get picked up instead. The kind mismatches for `IO` / `F` / `StreamIO` are the higher-kinded inference gap above

### Getting slick's 7 generated files (`.fm` templates) to compile

slick generates `TupleSupport` / `TupleShapeImplicits` / `SetParameter` / `GetResult` and others from FreeMarker templates at build time. Including these in the measurement (177 → 184 files) brought out a whole batch of gaps we had not seen. The minimal reproduction is in `tests/fixtures/genrep.scala` (the error cases are `genrep_bound_bad` / `genrep_tuple_bad` / `genrep_product_bad`), and `crates/cli/tests/genrep.rs` **compiles with both scalac and scala-rs and compares the output of `Main`**.

**1. Class type parameter bounds did not see imports**. Writing `class Boxed[T <: Rep[_]]` under `import slick.lifted._` produced `not found: type Rep`. The cause was that **the namer was resolving the bounds**. The namer runs before import clauses are processed, so imported names cannot be looked up at that point. Type parameters of a `def` worked because `type_def_sig` calls `enter_tparams` again, and type members worked because `type_type_member` does; only classes were never re-resolved. The namer now resolves them **provisionally and silently** (`enter_tparams_provisional`), and `type_class` re-invokes `resolve_tparam_bounds` in a scope where the imports are visible. A bound that genuinely does not exist is reported there (exactly once).

**2. The synthetic conversion for `implicit class` dropped the type parameters**. In nsc, `implicit class RepOps[T <: Rep[_]](c: T)` desugars to `implicit def RepOps[T <: Rep[_]](c: T): RepOps[T] = new RepOps[T](c)`. We used to synthesize it without type parameters, so the result type was a bare `RepOps` and it failed with `RepOps takes type parameters`. We now **copy the class's type parameter trees with fresh symbols** (`copy_tparams`), attach them to the synthetic `def`, and build `new C[T1, …](x)`.

**3. `TupleN` was neither a `Product` nor `Serializable`**. The prelude builds `Tuple2` in `prelude.rs` and `Tuple3`…`Tuple22` in `prelude_tuple.rs`, but their only parent was `AnyRef`. So neither `def buildTuple(…): Product = … new Tuple4(a, b, c, d)` (from the generated `TupleSupport`) nor even a plain `val p: Product = (1, 2)` compiled. `scala.Product` and `java.io.Serializable` live on the jar side (loaded on demand), so we pre-read just those two right after the classpath is installed and then wire up the edges (`prelude_genrep.rs`). **If there is no jar we do nothing**: the private runtime's `scala/Tuple2` implements neither, so claiming those parents would be a lie. Under `--no-scala-library` we report as before (`genrep_product_bad`).

**4. Inherited overloads lost the receiver's type arguments**. `scala.collection.Seq[A]` has two `apply` overloads: `SeqOps.apply(Int): A` and the `apply` inherited from `PartialFunction[Int, A]`. `Type::Overload` carries only types, and `resolve_overload` **re-read the declaration from the symbol** in order to learn the chosen candidate's symbol, so the second one reverted to the plain declaration `apply(A): B`, neither was more specific than the other, and we got `ambiguous overload for apply`. We now stash the already-computed "type at the receiver" in `overload_member_types` and use that on the re-read (so `s(0)` now works).

**5. Tupling of argument lists (nsc's tuple adaptation)**. `Some((p._1, p._2), p._3)` means `Some(((p._1, p._2), p._3))`. As a last resort we repack an argument list that fits no candidate into a single tuple and type it **exactly once more**. If that fails too we restore both the tree and the diagnostics, so the error reported is the one for what was written. A re-entrancy flag stops the synthesized `TupleN(a, b)` from re-entering. The handling of overloaded callees was fixed in `agent/hkinfer` (see "Base types of arguments and automatic tupling" below). We initially said "never apply this to overloads", but then `println(1, "a")` does not compile. The correct rule is: **do not tuple if even one candidate takes the number of arguments that was written**.

**6. Classes whose names merely start with `Tuple` were treated as tuples**. `TupleShape[L, M, U, P]` (a slick class of its own) was read as a **4-element tuple**, wiping out `TupleShapeImplicits`. We replaced `starts_with("Tuple")` / `starts_with("Function")` with a check that **the N of `TupleN` / `FunctionN` actually matches the argument count** (in both the typer's type resolution and the backend's pickle).

**7. Varargs constructors**. Passing `new C(a, b)` to `class C(xs: T*)` produced `type mismatch; found: a  required: T*`. The method side expanded repeated parameters via `param_at`, but the constructor side looked arguments up **by position**. slick's `new SetTupleParameter[(T1, T2)](c1, c2)` is this case. We fixed the codegen side too: on the JVM a repeated parameter is a single `Seq` argument (the `<init>` descriptor was `Lscala/collection/immutable/Seq;` all along), so pushing the elements raw gives a `VerifyError`. We now wrap them by going through the same `gen_call_args` as a method call.

**8. Wildcard type arguments and contravariance**. Given `SetParameter[-T]`, `SetParameter[T1]` conforms to `SetParameter[_]`. A wildcard denotes "some type", so it **contains** the other side regardless of the parameter's variance. Only in the contravariant case did we go look at `_ <: T1` and reject.

**9. Top-level definitions after `package p { … }`**. Writing `object Main` after `package genrep { … }` put `Main` into the `genrep` package and emitted `genrep/Main.class`. What follows the closing brace is a **sibling**, not a member of the package.

Measurement (`tests/slick_measure.sh`, 184 slick files, `-Xsource:3`) shows **2064 → 1300**, with errors in the 7 generated files going **736 → 41** (`TupleSupport` 569 → 2, `TupleShapeImplicits` 65 → 0, `SetParameter` 46 → 4, `GetResult` 25 → 4).

**Remaining** (not fixed in this slice):

- **The field type of varargs**. In nsc the `xs` of `class C(val xs: T*)` is `Seq[T]`, but here it stays `T*`, so `c.xs.length` gives `value length is not a member of T*` (we do not silently accept it). Constructor **calls** do now work.
- **The private runtime (`--no-scala-library`) still has no backing for varargs**, as before. Both `def f(xs: Int*)` and `class C(xs: T*)` reference `scala/collection/immutable/Seq`, so they give a `NoClassDefFoundError` at runtime (this is a pre-existing gap unchanged on the method side; constructors were merely brought in line with it).
- ~~**Case class constructor arguments do not implement abstract members**~~ → fixed in the `agent/ctoraccessor` slice. See "Constructor argument accessors and `FunctionN.tupled`" below.
- **`Vector[T]` does not conform to `scala.collection.IndexedSeq[U]`** (the `immutable.Vector` → `collection.IndexedSeq` edge is missing). There is also a gap where **explicit type arguments** such as in `Vector[Any](1)` are not propagated to the companion's `apply`.
- **`ClassTag` for tuple types**. The implicit for `classTag[(_, _)]` is not found; the 2 remaining errors in `TupleSupport` are this.

### Constructor argument accessors and `FunctionN.tupled`

The `agent/ctoraccessor` slice. The fixtures are `tests/fixtures/ctacc*.scala` and the test is `crates/cli/tests/ctoraccessor.rs`.

**1. `case class` constructor arguments did not become accessors**. This was the kind of gap that **passes typechecking but fails at runtime, and does so silently**.

```scala
trait Rep[T] { def value: T }
case class ConstRep[T](value: T) extends Rep[T]   // AbstractMethodError at runtime
```

For `class C(val x: Int)`, `emit_ctor_val_getters` emitted `x()`, but the test was only "did the parser see the `val` keyword and set `Flags::ACCESSOR`". A `case class` makes its **first parameter list `val` without the keyword**, so it did not go through there. Only the field was emitted, with no `value()`, and calling through `Rep` gave an `AbstractMethodError`. nsc's rule is "the first parameter list of a case class only" (the second and later lists stay private state; nsc rejects `case class C(implicit x: Int)` outright, so the first list is always non-implicit), and we added exactly that condition. When the parent erases to `def value: Object`, the existing bridge path is used as is. We cross-checked against real scalac 2.13.16 with `javap -p -s`, and `ctacc_case_class_params_get_public_accessors` pins down that the accessor names, descriptors and presence of bridges all match.

**2. `FunctionN.tupled` / `curried` and `scala.Function.untupled`**. slick's `generated/slick/lifted/CompilableFunctions.scala` builds 21 kinds of `CompiledFunction` with `f.tupled`, so arities 2 through 22 were all broken. The cause was that a function type (`Type::Function`) had no symbol for `class_sym_of` to return, so member lookup had nowhere to go. We redeclared `scala.FunctionN` with type parameters `T1 … Tn, R` (`prelude_fntuple.rs`) and made `type_select` look there only when the receiver is a function type (substitution matches the receiver's parameter types plus result type by position). `prelude.rs` just calls one line. `tupled` / `curried` are default methods on `scala/FunctionN` and `untupled` is on `scala/Function$`, so this is **restricted to `library_abi`**. The private runtime's `scala/Function0` / `Function1` only have `apply`, so under `--no-scala-library` we report `value tupled is not a member of (Int, Int) => Int` (`fixtures_ctacc_fn_without_library_is_error`).

Along the way we also fixed **three general gaps** (all reproducible without `tupled`):

- Calling `def g: Int => Int` as `g(3)` gave `no matching overload`. If the result of a method with no parameter list is a function, the argument list belongs to that function (`auto_apply_nullary_function`).
- `add(1)(2)` (a curried **function value**) collapsed into a single `Function1.apply`. Both uncurry and the backend have apply flattening, and neither considered "if the result of the inner Apply is a function type, it is a separate call". `Function.untupled(f)(1, 2)` is the same shape.
- Erasure was reading "the result type of the symbol the callee tree carries, even though the tree's type is a function type" and wrapping an unbox around it (the symbol of `f.tupled(t)` is `tupled`, whose result is the very function now being applied).

The four overloads of `Function.untupled` differ **only in the arity of the argument tuple**, so we narrowed overload scoring, which used to treat any two function types as unconditionally matching, to "if the argument side's parameter types are not still uninferred (a `{ case … }` literal), compare arity and tuple arity".

**3. `Builder`'s `+=` / `++=`**. `scala.collection.mutable.Builder` is not declared by the prelude and arrives via pickle supply. `b ++= xs` did not work for two reasons: `try_rewrite_assignment_op` (nsc's `convertToAssignment`, which rewrites `x += 1` into `x = x + 1`) decided "no such member" **without consulting the pickle**, and pickle supply refused `Growable`'s `+=` / `++=` returning `this.type` as an "unrepresentable type". For the first we now also try completion, and for the second we map `this.type` to **the receiver applied to its own type parameters** (`type_select` then fills in the receiver's type arguments, so for `Builder[Int, List[Int]]` we get `Builder[Int, List[Int]]` back, and it connects all the way through to `.result()`).

Measurement (`tests/slick_measure.sh`, 184 slick files, `-Xsource:3`) goes **1279 → 1219**, and files containing errors go **109 → 107**. Errors in `CompilableFunctions.scala` go 21 → 0, and `++= is not a member of Builder` goes 6 → 0.

**Remaining** (not fixed in this slice):

- **Visibility of constructor fields**. nsc uses `private final`; we use `public final` (accessors and bridges match). The codegen for pattern matching `getfield`s the same-named field directly, so making it private would require moving that path to an accessor call.
- **`Vector.newBuilder` / `List.newBuilder`** are not on the companions, so a `Builder` instance has to be written by hand (`ctacc_builder.scala` does that).
- **The `ClassTag` for `xs.toArray` is sometimes not filled in**. slick's `ProductResultConverter` (6 cases that call `cha(i)` with `(ClassTag[B])Any` still in place) remains.
- **The diagnostic for reading a constructor argument (without `val`) from outside** differs from nsc. nsc says `value hidden is not a member of Plain`; we say `value hidden cannot be accessed as a member of Plain from Main$` (both are errors; `ctacc_plain_bad`).

### Making case classes `Product`s (`agent/product`)

The `agent/product` slice. The fixtures are `tests/fixtures/prod*.scala` and the test is `crates/cli/tests/product.rs`.

Only `productPrefix` and `productArity` were synthesized, and `scala.Product` was not attached as a parent, so case classes were **half-way to looking like a `Product`**. All six of the following, which real scalac accepts, were failing.

```scala
case class P(x: Int, y: Int)
val p = P(1, 2)
p.productIterator.toList     // value productIterator is not a member of P
p.productElement(0)          // same as above
p.productElementName(0)      // same as above (added in 2.13)
P.tupled((5, 6))             // value tupled is not a member of P$
P.curried(5)(6)              // same as above
(p: Product).productArity    // type mismatch; found: P required: Product
```

**Rather than guessing what to emit, we read scalac 2.13.16's classfiles with `javap -v -p`** and decided from that. The rules we read off are recorded verbatim in the doc comment of `crates/typer/src/prelude_product.rs`.

**1. `case class` / `case object` are `scala.Product with java.io.Serializable`**. This is unconditional. A case class with a parent gets them appended after it (`class E$L implements E$T, scala.Product, java.io.Serializable`). Without these edges neither `val p: Product = P(1, 2)` nor `List[Product]` compiles, and `productIterator` / `productElementNames` have nowhere to come from (of the four, nsc overrides only `productIterator` / `productElement` / `productElementName` / `productPrefix` / `productArity` on the case class side, and inherits `Product`'s default implementation of `productElementNames`).

**2. `productElement` / `productElementName` are emitted ourselves**. Both are a **`tableswitch`** over `0 … arity-1`, and even a single field produces a table (`tableswitch { // 0 to 0 }`). For a case class with zero fields there is no switch at all and only the out-of-range path remains. Out of range is `scala.runtime.Statics.ioobe(I)` (i.e. `throw new IndexOutOfBoundsException(String.valueOf(i))`), and for `productElementName` a `checkcast java/lang/String` follows. When a field is a value class we **re-wrap it in an instance** before returning it, just like `toString` does (`new G$Meters(this.m())`).

**3. `productIterator` is an override, not inherited**, and calls `ScalaRunTime$.MODULE$.typedProductIterator(this)`. `productElementNames`, conversely, is a **mixin forwarder** to `Product`'s default implementation (`invokestatic InterfaceMethod scala/Product.productElementNames$`).

**4. `productElementName` on a `case object` is the one exception**. nsc does not synthesize `productElementName` for a case object, so the module class carries a forwarder to `Product`'s default implementation instead. That default has a different message: `productElementName(0)` on `case class Zero()` throws `IndexOutOfBoundsException: 0`, whereas `case object Solo` throws `IndexOutOfBoundsException: 0 is out of bounds (min 0, max -1)`. **Two different messages appear within the same program**, so we reproduce both exactly (the last 4 lines of `prod.scala`).

**5. The companion extends `scala.runtime.AbstractFunctionN`**. That is where `tupled` / `curried` come from (default methods on `FunctionN`; on the prelude side, `prelude_fntuple.rs` puts the same two on `FunctionN`). We used inheritance rather than growing the methods directly because that is **what the real thing does**. As a bonus, `val f: (Int, String) => P = P` and `List(1, 2, 3).map(One)` now work too. All four conditions for inheriting it were likewise read off the classfiles.

- **It is not attached to an `object P` you wrote yourself**. Whatever it extends is irrelevant: `object P extends Base` gives `class F$Plain$ extends E$Base`, and even `object P extends SomeTrait` gives `class F$WithTrait$ implements E$Mix` with `AbstractFunction1` nowhere to be seen (though `java.io.Serializable` is attached).
- **It is not attached to a case class with type parameters**. `case class Gen[A](a: A, b: Int)` gives only `class E$Gen$ implements java.io.Serializable`.
- **It is not attached when there are two or more argument clauses**. An implicit clause counts too (both `case class Impl(a: Int)(implicit o: Ordering[Int])` and `case class Curr(a: Int)(b: String)` are plain `Serializable`).
- **It is not attached for arity 23 and above**. `AbstractFunctionN` only goes up to 22, and a sibling with exactly 22 gets `AbstractFunction22`.

It is attached to a case class with varargs (`case class Vararg(a: Int, rest: String*)` → `AbstractFunction2<Object, scala.collection.immutable.Seq<String>, F$Vararg>`). Since it extends `AbstractFunctionN`, it must implement the erased `apply(Object, …)Object`, so we emit that bridge on the companion as well (nsc emits it in the same place).

**All of this is gated on `library_abi`**. `scala.Product`, `java.io.Serializable`, `scala.runtime.AbstractFunctionN`, `scala.collection.Iterator` and `scala.runtime.ScalaRunTime` are all on the jar side, and the private runtime (`crates/backend/src/runtime.rs`) has none of them. Under `--no-scala-library` we do not wire up the parents, and `p.productIterator` is still reported as `value productIterator is not a member of P` (`fixtures_prod_lib_without_library_is_error`). However, `productElement` / `productElementName` only need `java.lang`, so we **emit them in both modes**. On the private runtime side we write out the same throw in place of `Statics.ioobe`, and for case objects the same message as `Product`'s default, inline. `prod.scala` and `prod_vc.scala` **match byte for byte across all three of the private runtime, the jar, and real scalac**.
### Overload candidate sets (inheritance, `private[this]`, `java.lang.String`)

The `agent/ovl2` slice. Fixtures are `tests/fixtures/ovl2*.scala`, tests are
`crates/cli/tests/ovl2.rs`. The cluster of `no matching overload` errors left in slick was
caused not by the **resolution rules** but by **how the candidate set was built**.

**1. Inheritance is not overriding.** `drop_overridden` merely dropped a candidate "if the
owner is a superclass", without looking at signatures.

```scala
class Base { def f(x: Int): String = "int:" + x }
class Derived extends Base { def f(s: String): String = "str:" + s }
new Derived().f(1)   // dropped Base.f(Int), hence: no matching overload
```

As in nsc's `matchingSymbols`, the parent's member is now dropped **only when the signatures
match** (argument lists are flattened and compared by count and by type; parameters that
involve type parameters or abstract type members are treated as matching without
reconstructing as-seen-from). Along with that, **constructors are not inherited**, so
`pick_ctor_at` excludes from the candidates the `<init>` that `lookup_member` picks up from
the parent.

The same gap existed in the backend. `emit_erasure_bridges` emitted a bridge merely because
"the name is the same but the descriptor differs", so for the `Derived` above it emitted an
**unverifiable** bridge `f(I)Ljava/lang/String;` (pushing an `Integer` where a `String` is
expected). A bridge is now emitted only when the parent's parameter erases to `Object`
(i.e. an implementation of a generic method). Pinned down with `-Xverify:all`.

**2. `private[this]` is not inherited.** A template's scope puts its own members and its
inherited members into the **same scope**, so plain constructor parameters (`private[this]`
in nsc) collided between parent and child. In slick,
`LoggingPreparedStatement(st: PreparedStatement) extends LoggingStatement(st: Statement)`
made `st` an `<overload Statement | PreparedStatement>`, and every `st.execute()` failed.
`enter_inherited_members` now refuses to enter members with `PRIVATE | LOCAL`. Conversely,
naming a parent's plain parameter from the child is an error, just as in nsc (`ovl2_bad`:
`not found: value tag`).

The **selection side** of the same rule (nsc's `nonLocalMember`) went in too. A
`private[this]` member is not a member of any prefix other than `this`, so for a selection on
another instance it is dropped from the candidates and the **inherited member** of the same
name is read instead.

```scala
class Sym(val name: String)
class Fun(name: String) extends Sym(name) {   // `name` is the constructor parameter
  override def equals(o: Any) = o match {
    case o: Fun => name == o.name             // `o.name` is the val in Sym
    case _      => false
  }
}
```

Because the plain parameter was masking the inherited member, `o.name` in slick's
`Library.JdbcFunction` / `SqlOperator` / `SqlFunction` produced
`value name cannot be accessed as a member of JdbcFunction from JdbcFunction`.

#### `private[p]` resolves outward from the **definition site**

The `X` in `private[X]` is the name of a class or package enclosing the definition. Looking
the name up in the scope of the use site made slick's `private[util] def copySliceTo`
(in `package slick.util`) hit `scala.util`, which turned every reference — even those from
the same package — into `cannot be accessed`. `X` is now resolved by walking outward from the
member's owner (`access_within_of` in `check.rs`). The package boundary itself is unchanged,
so `mism8_access_bad.scala` is still rejected as before.

**3. A `val` implementing an abstract `def` is one member.** For the same reason, a
`val symbolName: SymbolNamer` implementing
`trait InterpolationContext { def symbolName: SymbolNamer }` became
`<overload SymbolNamer | SymbolNamer>` and `symbolName(s)` did not resolve. `bind_found`
(the identifier side) now runs `drop_overridden` too.

**4. `java.lang.String` implements `CharSequence`.** The prelude gave `String` only `AnyRef`
as a parent, so `String <: CharSequence` was false and **every** JDK overload taking a
`CharSequence` was inapplicable (`Instant.parse(s)` / `LocalDate.parse(s, fmt)` /
`DateTimeFormatter.parse(s)`). `prelude_strhier.rs` reads `Comparable` / `CharSequence` /
`Serializable` from the JDK and adds them as parents, and `is_sub_type` follows them. This is
a fact about the JVM, so it is effective in both modes, **independently of `library_abi`**.
The same file adds the `(Int)` / `(Int, Int)` / `(String, Int)` overloads of `indexOf` /
`lastIndexOf` (`s.indexOf(':')` widens `Char` to `Int` and picks `indexOf(int)`, so without
that candidate it fails).

**5. Eta-expansion of overloaded methods.** When the expected type is a function type, nsc's
`inferExprAlternative` narrows to "the one alternative that can be eta-expanded to that
function type". To make both `constOp[Long]("min")(math.min)` and
`val g: (Long, Long) => Long = math.max` work (argument position and expected-type position),
`pick_overload_for_function` was added to `adapt`, and the scoring side now treats a
`Type::Overload` argument as matching if **any one** alternative matches.

**6. `new ArrayBuffer[R](g.length)`.** The prelude did not declare `ArrayBuffer`'s
`def this()` / `def this(initialSize: Int)` (`prelude_ovl2.rs`). Both exist as `<init>()V` /
`<init>(I)V` on the real 2.13.16 class.

**7. Proper-subclass relation of the declaring classes (nsc's `isInProperSubClassOf`).** When
each alternative is as specific as the other, nsc's `relativeWeight` picks **the one whose
owner is a proper subclass**. 2.13's `SortedSetOps.map[B](f)(implicit ord)` and
`IterableOps.map[B](f)` are exactly that case, and without this rule `TreeSet.map(f)` was an
`ambiguous overload` (nsc's `isAsSpecific` ignores the implicit clause, so looking only at the
explicit parameters the two are equally specific). See the section "Higher-order expected
types... (slice 9)" for details.

The measurement (`tests/slick_measure.sh`, 184 slick files, `-Xsource:3`) went from
**1059 to 903**, and files containing errors from **105 to 104**.

**What remains** (not fixed in this slice):

- **`T` cannot be solved from `Map[K, V] <: Iterable[T]`.** The `no matching overload` on
  `ConstArray.from(m)` is not about the candidate set but about `infer_method_tparams`; it
  reproduces by simply passing a `Map[String, Int]` to `h[T](xs: Iterable[T])`
  (`h2(xs: Iterable[(String, Int)])` and the explicit `h[(String, Int)](m)` both work, so it
  is inference, not conformance). This is `agent/tyvar`'s territory, so it was left alone.
- **JDK members of `java.lang.String` are not loaded on demand.** Only what the prelude
  declares exists, so `codePointAt` and friends give
  `value codePointAt is not a member of String`.
- **The `ClassTag` for `xs.toArray`** (slick's `ProductResultConverter`, `(ClassTag[B])Any`)
  is still a remaining item from `agent/ctoraccessor` above.

### Type members, `this.type`, and cleaning up undetermined variables (`type mismatch`, slice 3)

The `agent/mismatch3` slice. Fixtures are `tests/fixtures/mism3*.scala`, tests are
`crates/cli/tests/mismatch3.rs`. Eight causes were fixed.

**1. The order in which inherited members were entered into scope was not the linearization.**
`enter_inherited_members` walked the parents **depth first**, so a *grandparent's* abstract
declaration was entered into the scope before its subclass's concrete declaration.

```scala
trait N { type Self >: this.type <: N; def self: Self }
abstract class Base[T] extends N { type Self = Base[T] }
trait Extra extends N
new Base[T] with Extra { def self: Self = this }   // Self was resolving to N's abstract declaration
```

The parents are now walked **breadth first in reverse order (last mixin first)**. That is
exactly the order in which members arrive under nsc's linearization, and for the direct
parents "the last mixin wins" as before. slick's
`new SimpleFeatureNode[T] with SimpleFunction { … }` was this case.

**2. The right-hand side of an alias type member was not as-seen-from'd.** The `T` in
`type Self = Base3[T]` is `Base3`'s own type parameter, so read through `Base3[String]` it
must be `Base3[String]`. The substitution now happens both on the name-resolution side
(`Check::type_member_here`) and in expansion through a receiver
(`SymbolTable::expand_in_type`).

**3. Type parameters that no argument could determine stayed as type parameters.** In
`def dbAction[R, S <: NoStream, E <: Effect](f: Session => R): ProfileAction[R, S, E]`, `S`
appears in no parameter type, so nsc fixes it at its bound in `solvedTypes` (the lower bound
if covariant, the upper bound if contravariant). We were reporting `Act[Unit, S, Schema]` as
is. `instantiate_leftover_tparams` was added. Type parameters that **do appear in a parameter
type** are out of scope for it (those are determined by an argument or an implicit, and
collapsing them would make diagnostics disappear; `exptype_unsolved_bad` still fails as
before). Calls with no expected type (receiver position) are also out of scope; there, as with
nsc's `Context.undetparams`, the decision is left to the following application.

**4. The block on the line after `new C with T { … }` became an argument.** nsc's `canApply`
was missing, so

```scala
def build(p: IndexedSeq[Node]): SimpleFeatureNode[T] = new SimpleFeatureNode[T] with SimpleFunction {
  …
}
{ (paramsC: Seq[Rep[?]]) => … }      // this became the argument to the anonymous class above
```

was parsed that way, `build` was eta-expanded as the value of the block, and the types did not
match. `parse_simple_expr` now sets `can_apply = false` immediately after `new` (and, as in
nsc, sets it back to true once a `.` or `[…]` has been followed).

**5. `this.type` dropped the receiver's type arguments.** Calling `def add(v: T): this.type`
on a `B[String]` produced `B` (no arguments), and the parameter of the next `add` became a
bare `T`. `subst_as_seen_from` now replaces `C.this.type` occurring in a member's signature
with the receiver itself. A self alias (the `self` in `trait T { self => }`) refers to the
**enclosing instance**, so it is not replaced.

**6. Undetermined variables carried over by the receiver could not be determined by the
arguments of that call.** `ConstArray.newBuilder()` is a `ConstArrayBuilder[?T]`, and the
`from` in `b + from` determines `?T`. A call's own type parameters left in its result are now
recorded in `undet_tvars` and solved from the arguments (this is the cluster of `+` / `++` in
slick's `Comprehension.children`).

**7. Protected access only looked at the innermost class.**

```scala
class DDL(val stmts: List[String]) { self =>
  protected def phase: List[String] = stmts
  def merge(other: DDL): DDL = new DDL(Nil) {
    override protected def phase = self.phase ++ other.phase   // "not accessible from $anon"
  }
}
```

nsc judges the rule against **every enclosing class**, so as long as it is written in the body
of `DDL`, a prefix of `DDL` is enough. `protected_subclass_ok` now walks the owner outward.
Along the way one backend gap was closed too: reading a self alias stopped at
`load_owner_instance`'s "if `this` conforms to the owner, use `this`", so inside an anonymous
class that **is also a subclass of the owner** it read `this`, and the `self.phase` above
called its own override and recursed forever (`load_self_alias_instance` now follows `$outer`
by identity).

**8. Classpath pickles discarded type arguments and kinds.** `unpickle` skipped the type
arguments of `TYPEREFtpe`, and a class's type parameters were names only. Using `Monad[F[_]]`
through `-cp <directory>` gave `kinds of the type arguments (F) do not conform`, and
`c.as(1)` came out as `Any`. `PickledType` / `PickledTypeParam` were introduced so that the
reader (`classpath.rs`) can turn `Function1[A, B]` / `Tuple2[A, B]` / `Array[T]` back into
structural `Type`s. The writer side now also writes a `POLYtpe` into the `TYPEsym` of a
higher-kinded type parameter (verified that the real scalac 2.13.16 can read it with `-cp`).
Classes from jars do not go through this path (they go through `adopt_binary_class`, covered
in the next section).

The measurement (`tests/slick_measure.sh`, 184 slick files, `-Xsource:3`) went from
**833 to 772**, `type mismatch` from **201 to 168**, and files containing errors from
**102 to 100**. No file started reporting errors that did not before.

### Premature alias completion and `FunctionN` (`type mismatch`, slice 4)

The `agent/mismatch4` slice. Fixtures are `tests/fixtures/mism4*.scala`, tests are
`crates/cli/tests/mismatch4.rs`. Six causes were fixed.

**1. A lazily completed type alias did not see its file's imports** (the largest cluster).
A type alias is completed the moment its name has to be dealiased. The parent clause of a
*nested template* does exactly that, so completion runs from the header pass (`parents_pass`)
before the signature pass reaches the alias.

```scala
import slick.sql.FixedSqlAction
trait JdbcActionComponent extends SqlActionComponent { self: JdbcProfile =>
  type ProfileAction[+R, +S <: NoStream, -E <: Effect] = FixedSqlAction[R, S, E]
  abstract class SimpleJdbcProfileAction[+R](…) extends … with ProfileAction[R, NoStream, Effect]
}
```

At that point the only record of the alias is the namer's, and the namer **does not save
scopes** (`PendingSig.scopes: None`). `swap_in_scopes` rebuilds the scopes from the owner
chain, so the enclosing template's members get in but **the file's imports do not**. As a
result `FixedSqlAction` stayed a `Type::Named` and could not be resolved, `ProfileAction`'s
type was pinned to `<error>`, and from then on every
`new SimpleJdbcProfileAction[Unit](…) { … }` gave
`type mismatch; found: $anon$N required: JdbcActionComponent.ProfileAction[…]`
(26 occurrences in `JdbcActionComponent` alone, plus the same thing in `MemoryProfile` /
`MemoryQueryingProfile`).

The header pass has **already typed that file's imports and finished entering the template's
members** — it is exactly the vocabulary the alias was written in. `refresh_alias_sigs` hands
the current scope stack to the `PendingSig` of any `TypeDef` that still holds only the namer's
record, just before descending into a nested template.

**2. A compound type did not conform to an "applied abstract type member".**

```scala
trait P { type M[+R] <: A[R];  type N[+R] <: A[R] with M[R] }
trait Q extends P { type M[+R] <: B[R];  type N[+R] <: B[R] with M[R] }
```

When checking `B[R] with M[R] <: A[R] with M[R]`, the `M[R]` on the right is an application of
an **abstract** member and has no right-hand side to expand. The `(other, Applied)` arm of
`is_sub_type` simply returned `false` there. When the right-hand side cannot decide, nsc falls
back to the **left-hand side's rule**, so a compound conforms through one of its own parents.

**3. `Map[K, V]` was not a `K => V`.** 2.13's `scala.collection.Map[K, +V]` extends
`PartialFunction[K, V]` (`scala/Function1` is right there in `javap`'s interface list). The
prelude's hierarchy table (`prelude_hier.rs`) only had the `Iterable` edges, so slick's
`val symbolToIndex: TermSymbol => Int = someMap` failed. The edges are now added in
`crates/typer/src/prelude_mism4.rs`.

At the same time, **the `scala.FunctionN` "class" and the structural `(T1, …) => R` are now
treated as the same type** (`SymbolTable::function_class_shape`). The prelude writes things
like `PartialFunction`'s parents as classes and uses the structural form elsewhere, so without
being able to move between the two, not even `PartialFunction[A, B] <: A => B` holds. This
matters in three places: both directions of `is_sub_type`, the expected type of a function
literal (`type_function`), and the applicability check for overloads (`arg_score`). The third
is important because **signatures coming from pickles write function parameters as classes**,
so passing a literal to
`IterableOnceOps.reduceLeft[B >: A](op: Function2[B, A, B]): B` gave
`no matching overload … with arguments ((<notype>, <notype>) => <notype>)`.

**4. `map` dropped the receiver's collection.** `IndexedSeq` does not redeclare `map`, so the
inherited declaration says `Seq[B]`. But the actual signature returns the receiver's own type
constructor (`IterableOps.CC[B]`), so if `xs.toSeq.map(f)` is on an `IndexedSeq` the result is
an `IndexedSeq` too. Previously "the declared result wins", full stop. The receiver now takes
priority, but only when **the receiver is a `scala.collection` class and a descendant of the
declared result class**. `Range` (which has no type parameter of its own) still gets the
declared `IndexedSeq` as before, and a user class that merely extends `Seq` inherits `Seq`'s
builder, so it too keeps the declared result.

**5. A stable identifier pattern was rejected by a scrutinee that was not yet determined.**

```scala
def f[T](t: ScalaType[T]) = t match {
  case ScalaBaseType.byteType => …    // found: ScalaNumericType[Byte] required: ScalaType[T]
}
```

`T` might be `Byte`, and at run time the pattern is just an `==`, so a scrutinee whose type
arguments are still unknown rules nothing out. `relax_abstract_targs` replaces type parameters
and abstract type members in the **type arguments** of the scrutinee used as the expected type
with `_` (the head class is not relaxed).

**6. `this` did not conform to `type Self >: this.type <: Node`.**

```scala
trait Node { type Self >: this.type <: Node; def mapChildren(…): Self }
trait NullaryNode extends Node {
  override final def mapChildren(f: Node => Node, keepType: Boolean = false): Self = this
}
```

`adapt_singleton` already had "if the **lower bound** of an abstract type member is
`this.type`, `this` is accepted", but its `ThisType(cls)` check used **identity** on
`tree.sym == cls`. The lower bound is written in `Node`'s vocabulary, so read from
`NullaryNode` it is `NullaryNode.this.type`. It now accepts the case where the class the
`This` tree points at is a **descendant** of `cls`. Because it **only applies to `This`
trees**, `def wrong(a: Node, b: Node): a.Self = b` still fails (scalac rejects it too) — the
worry that "adding the lower-bound rule naively would let a different `Node` through" is cut
off right here. Compounds such as `Node.Self with DefNode` are now examined one parent at a
time as well. In `val n: Self = if(…) this else rebuild(…)`, once `this` is accepted it widens
to `Self` (sound, since `this.type <: Self`), so the lub of the two branches is `Self` too.

The measurement (`tests/slick_measure.sh`, 184 slick files, `-Xsource:3`) went from
**711 to 635**, and files containing errors from **91 to 87**. No file started reporting errors
that did not before. `type mismatch` went from **157 to 127**, but that includes the effect of
item 3: things that used to be `no matching overload` turned into the `type mismatch` they
really were (the cats-effect area of `BasicBackend`, among others). Looking at `type mismatch`
alone, it dropped from 157 to 114, then went back up to 131 once the `Function2` gap was
closed, and ended at 127 with `Self`.

**What remains** (not fixed in this slice):

- **`case Seq(a, b)` is unusable.** Only the prelude's `List` has an `unapplySeq`, and `Seq`
  does not, so `case Seq((s, _)) => Some(s)` degrades to a "class pattern" and the elements get
  no type (4 occurrences in slick's `JdbcStatementBuilderComponent`). Adding it to the prelude
  is easy, but codegen's `gen_unapply_seq_bind` is **List-specific**, starting from a
  `checkcast List`, so passing a `Vector` as a `Seq` fails at run time. It needs either a
  version using `SeqOps.length` / `apply(I)` or an inserted `toList`. Incidentally, codegen for
  `case List(a, b, rest @ _*)` emits a `VerifyError` **even today** (no checkcast is emitted for
  the elements before the star pattern).
  → **Solved in `agent/seqpat`** (below). The `VerifyError` for `List(a, b, rest @ _*)` had
  already been fixed in the earlier `41d4bca`.
- **`StringOps.map[B](f: Char => B): IndexedSeq[B]`** is missing, so
  `"…".map(_.toString)` gives `found: String required: Char`. 2.13 has two overloads, along
  with `map(Char => Char): String`, but putting both in the prelude yields an
  `ambiguous overload` before the literal's result type is known, and folding them into one
  makes codegen call the one returning `IndexedSeq`, because erasure re-fetches the result type
  from the symbol. This is on hold until overload resolution can narrow using a literal's
  result type. → **Solved in `agent/seqpat`** (below).
- **Typechecking of stable identifier patterns itself.** scalac 2.13.16 **accepts**
  `case Ids.other =>` (with `other: Other` and scrutinee `ST[Int]`), whereas we still report a
  `type mismatch`. This time the rule was relaxed only when the type arguments are abstract.
  → **Solved in `agent/seqpat`** (below).
- `Seq("a").map(m)` (with `m: Map[String, Int]`) still fails even now that `Map` is a function.
  That is the inference side (the `B` of `Function2[B, …]` is unsolved), not conformance.

### Traits extending a function type, and omitted type arguments (`type mismatch`, slice 5)

The `agent/mismatch5` slice. Fixtures are `tests/fixtures/mism5*.scala`, tests are
`crates/cli/tests/mismatch5.rs`. Eight causes were fixed.

**1. A trait with a function type as a parent did not become a SAM** (the largest cluster).

```scala
trait CanBeQueryCondition[-T] extends (T => Rep[?])
implicit val c: CanBeQueryCondition[Rep[Boolean]] = value => value
```

The single abstract method is `Function1.apply`, and it is inherited from a **structurally
written parent**. `class_sym_of` deliberately does not turn a `Type::Function` into a class
(so that conformance and erasure treat it structurally), so **only the places that need a
class** now call `SymbolTable::function_class_form` (the inverse of `function_class_shape`):
SAM search (`abstract_sam_methods`), member lookup (`lookup_member`), as-seen-from (the `walk`
in `subst_as_seen_from`), the JVM interface list (the backend's `split_parents`), and
linearization (the backend's `linearize`) — five places.

At the same time, **the prelude's `FunctionN` got real type parameters and its `apply` was
made `ABSTRACT`**. Previously it was `apply(Any): Any` and non-abstract, so (a) the SAM search
found no abstract method at all, and (b) even if it had, there was nothing to substitute when
reading it through `C[X]`. For the sake of `self.apply(rs)`
(`trait GetResult[+T] extends (PositionedResult => T) { self => }`), `walk` also walks
`ThisType`. The `Type::Class` arm of `resolve_overload` now does as-seen-from just like
`type_select` does (reading it raw gave `found: 3 required: T1` for `m(3)`). And when a
`Select` **resolves to a value**, it becomes a receiver into which `.apply` is inserted
(previously we gave up merely because it was a Select).

**2. The second inference pass discarded "a type parameter of the caller" as a solution.**

```scala
def mk[T](f: PR => T): GR[T] = …
def const[T](value: T): GR[T] = mk(_ => value)   // found: GR[T] required: GR[T]
```

`mk`'s `T` can only be determined from the lambda's **result**, so the second pass solves it,
but that pass rejected every `Type::TypeParam`. What should be rejected is only **the call's
own** variables (`T := T` is not a solution); a type parameter of the caller is a perfectly
good solution.

**3. `extends Base(s)` did not infer the parent's type arguments.**

```scala
class DerbySequenceDDLBuilder[T](seq: Sequence[T])
  extends SequenceDDLBuilder.BuiltInSupport.OverrideActualStart(seq)
```

nsc's `parentTypes` infers the parent's type arguments from the constructor arguments. Without
that, the parameter stayed as `Sequence[Base.this.T]`, both sides printed as `Sequence[T]`, and
the diagnostic said neither was the other. The inferred type arguments also become the
**recorded parent**, so `Derived[X] <: Base[X]` holds too (`Typer::infer_parent_targs`).

**4. `new C` did not read type arguments from the expected type.**

```scala
def unit[R]: ResultConverter[R, W, U, Unit] = new UnitResultConverter
```

The expected type names a **parent class**, so `R` is read from
`UnitResultConverter[R] <: RC[R, …, Unit]` (the same computation a constructor pattern does
against a scrutinee, i.e. `base_targs_from_pt`). Since it has to be combined with what the
arguments can solve, it returns an `Option` per parameter. Along with that, **the head of
`new C(args)` no longer has to conform to the expected type of the whole application**
(`type_expr_inner`). That was what made us look at the head alone and say
`found: ProductResultConverter required: ResultConverter[R, W, U, _]`.

**5. Arguments were not aligned to the parameter's class before unification.** `unify_one` has
no symbol table and zips type arguments positionally, so passing a `UnitRC[String]`
(i.e. `RC[String, Unit]`) to `def id[R, U](c: RC[R, U])` zipped `[String]` against `[R, U]` and
left `U` unsolved. `unify_tparam_all` now aligns them via `base_type_instance`.

**6. An implicit-only argument clause was not filled in from the expected type.**
`TreeMap.empty` is `[K: Ordering, V]: TreeMap[K, V]`, and `V` appears in no implicit parameter.
So search alone cannot determine it, and `adapt_implicit_apply` did nothing — "waiting for a
`TypeApply`" — leaving **the method type itself** as the value's type
(`found: (Ordering[K])TreeMap[K, V]`). nsc runs `inferExprInstance` first, so we now go ahead
**whenever the expected type can determine all the type parameters that appear in the implicit
parameters**.

**7. `.apply` was not inserted on an annotated type.** In slick, the `m(f)` following
`val (b, m: Map[…] @unchecked) = …` gave
`value apply is not a member of Map[…] @unchecked`. An annotation says nothing about **what
members a type has**, so every place that asks about shape now sees through annotations
(`strip_annotations`).

**8. Transformations preserving the element type did not return the receiver's collection.**
2.13 declares `filter` / `filterNot` / `take` / `reverse` / `++` / `:+` / `updated` /
`sortWith` and others as returning `C` (the receiver's own collection). The prelude cannot
write `C`, so `Vector[Phase].filterNot(p)` came out as the inherited `Seq[Phase]`, and
`phases ++ ps` as `IndexedSeq[Phase]`. It is the same shape of rule as `map` in slice 4, but
**restricted to members whose post-erasure descriptor returns `Object`** (`erases_to_object`).
`TreeMap.filter` returns a `Map` on the JVM, so narrowing it to `TreeMap` here would make
codegen push a `Map` into a `TreeMap` field and cause a `VerifyError` (the `to*` conversions
were out of scope from the start, i.e. `v.toSeq` really is a `Seq`).

As a bonus, **the element type of a collection factory is now widened by the expected type**.
`Set` and `Map` are invariant, so `def f(s: AnonSym): Set[Sym] = Set(s)` is not a subtyping
problem: the factory's shortcut (determining the element type from the arguments alone) has to
ask the expected type (`factory_targs_from_pt`).

The measurement (`tests/slick_measure.sh`, 184 slick files, `-Xsource:3`) went from
**620 to 547**, files containing errors from **87 to 81**, and `type mismatch` from
**127 to 98**. No file started reporting errors that did not before.

> A note on measurement: the `BIN` in `tests/slick_measure.sh` points at **the parent
> repository's** `target/release/scala-rs`. When working in a git worktree,
> `cargo build --release` writes into the worktree's own `target/`, so the script measures
> `main`'s binary rather than the one you built (we hit this in the very literal form of
> "the numbers don't move a millimetre no matter what I change"). In a worktree, say
> `SCALA_RS=<worktree>/target/release/scala-rs tests/slick_measure.sh` explicitly.

**What remains** (not fixed in this slice):

- **`-` / `removed` / `incl` / `excl` / `filter` on `MapOps` / `SetOps` cannot be narrowed to
  the receiver's collection.** On the JVM these return the **named classes** `Map` / `Set`, so
  even if the typer narrows to `TreeMap`, codegen re-fetches the Apply's result type from the
  erased symbol, pushes a `Map` into a `TreeMap` field, and produces a `VerifyError`. The
  restriction can be lifted once the Apply's own result type survives erasure (an area
  `agent/seqpat` is touching). Two occurrences remain in slick's `ConcurrencyControl`.
- **Writing types on the components of a tuple pattern definition gives a `VerifyError`**
  (same on main). `val (n: Int, s: String) = if (b) (1, "x") else (0, "y")` gives
  `Bad local variable type` (an int placed in a reference local). slick's `HoistClientOps` has
  `val (bl2: Bind, lrepl: Map[…] @unchecked) = …`, which is this shape.
  → **Solved in `agent/mismatch6`** (a `_: T` subpattern is bound as a reference).
- **Propagation of the expected type into tuple components.** Typing
  `(new Sel, Map(s -> a))` against `(Node, Map[Sym, Int])` types the component `Map(s -> a)`
  with no expected type, giving the invariant `Map[AnonSym, Int]`. We tried adding nsc's
  `protoTypeArgs` (which forms an expectation of the type arguments from the expected type
  before typing the arguments), but by-name parameters were then passed as `() => T` and the
  count got worse, 611 to 604, so it was rolled back. A version excluding by-name parameters
  looks likely to work.
- **`def wrong[A, B](v: B): GR[A] = mk(_ => v)` is accepted** (same on main). After the
  expected type forces `T := A` in an invariant position, the lambda's body is not rechecked
  against `A`. scalac reports `found: v.type required: A`.

### Lubs, and three cases that pass typechecking and then fail (`type mismatch`, slice 6)

The `agent/mismatch6` slice. Fixtures are `tests/fixtures/mism6*.scala`, tests are
`crates/cli/tests/mismatch6.rs`. It fixed the **three cases** recorded under Remaining in the
README (two of them codegen bugs that pass typechecking and then produce a `VerifyError`, one
an explicitly typed lambda that fails typechecking) plus six causes of `type mismatch` — nine
items in all.

**1. The lub of branches came out as `java/lang/Object`** (codegen).

```scala
h.cur = (3: Int) match { case 0 => None; case n => Some(n) }
```

The `match` branches push **different classes**, `scala/Some` and `scala/None$`. The assembler
has no class hierarchy, so `merge_vtype` collapsed the two to `java/lang/Object`. The result
was that `putfield Lscala/Option;` gave `VerifyError: Bad type on operand stack`.

**The expression's own static type is the upper bound of every branch**, so the generator now
hands it over (`Assembler::set_join_class`; `gen_match` / `gen_int_switch` / `gen_if` declare
`join_class_of(result_ty)` on the join label). The join is applied only to the top of the stack
— that is, to the value actually being joined. What is below was pushed before the branch and
is the same on every path.

`try` puts its result in a **local**, so it needs the same treatment
(`Assembler::set_local_class`; every reference stored into the declared slot is recorded as
that class).

At the same time `ret_object` was **removed**. It was a guess put there purely for `areturn`:
"a join of references is **the method's return type**". Nothing guarantees the declared type is
the real upper bound, and when a `String` and an `Integer` were joined **inside** a method
returning `Option`, as in `Some(n match { case 1 => "one"; case _ => n })`, the frame claimed
`scala/Option` and gave `VerifyError: Inconsistent stackmap frames` (same on main). A join with
no declaration is now `java/lang/Object` — a frame type that is always correct, since any
reference is assignable to it — and only the places that know the real type say so via
`set_join_class`.

**2. The type of a `try` stayed the type of its body.** This is the typechecking-side gap
paired with the above.

```scala
try Success(f) catch { case NonFatal(e) => Failure(e) }
```

The comment said "nsc takes the lub of the body and the handlers", but the implementation took
the lub only when the body was `Nothing`, and otherwise used the body's type as is. When the
handler **does not conform** to the body, the lub is needed. `try n catch { case _: Exception => "x" }`
is `Any`, not `Int`, and leaving it as `Int` made us `istore` an `Integer` into an `int` slot
and produce a `VerifyError` (same on main).

The result goes into a single local (a single JVM sort), so if a primitive arrives at a
reference slot it is boxed (`box_for_result_slot`). Whether a branch has already been boxed is
not visible from the tree's type — the typechecking-side adapt sometimes boxes it — so we look
at **the actual type on the assembler's stack** (`Assembler::top_is_reference`).

The only shape where no lub is taken is when a branch is `Unit`. nsc lubs a statement-position
`try f() /* Int */ catch { println }` to `Any`, but `gen_try` already has a path for that shape
that pushes the default value for the body's sort.

**3. A `_: T` subpattern was not kept as a reference** (codegen).

```scala
val (n: Int, s: String) = if (b) (1, "x") else (0, "y")
```

After reading `Tuple2._1`, `emit_from_erased_object` unboxed it to `int`, and the following
type test then did an `aload` on that local and gave
`VerifyError: Bad local variable type`. `_: T` is a **test**, so it needs a reference for
`instanceof` to read — the unboxing is now done in the `Typed` arm of `gen_pattern` after the
test has passed. The seven places that bind subpatterns (`bind_subpattern`) now take **the sort
of the value on the stack** rather than the pattern's type. If the scrutinee is already a
primitive the type test is statically decided, so the `Typed` arm passes it through without
emitting an `instanceof`.

**4. The body of an explicitly typed lambda was not checked against the expected result type.**

```scala
xs.foreach((x: Int) => x + 1)   // found: (Int) => Int  required: (Int) => Unit
```

nsc types a function literal's body against the **result** of the expected type (value
discarding and numeric widening both happen there). A literal with written parameter types was
typed **before the expected type was known**, because overload resolution needs its result
type, and so the body never saw the expected result type. `adapt_function_literal_result` was
added to `adapt`, adapting the body to the expected result type — **only for literals**. Only
for literals: `val h: Int => Int = …; fu(h)` is still a `type mismatch`, as in nsc
(`tests/fixtures/mism6_bad.scala`).

**5. A `Map` is the function it declares itself to be.** In 2.13,
`MapOps[K, +V, …] extends IterableOps[…] with PartialFunction[K, V]`, so
`on.map(columnIndexes)` is a key lookup. The prelude has no `MapOps`, and giving `Map` a
`PartialFunction` parent changes the traversal order of inherited members and breaks
`toMap`'s `A <:< (K, V)` (`A` becomes `Char`), so the fact was written into
`Typer::function_view` (which reads `arg` as an inherited structural function type). It is used
in three places, in every case as a fallback for **when nothing else determined anything**:

- at the end of `arg_score` (scoring it earlier made slick's `map` calls come out as
  `ambiguous overload` across the board)
- `unify_tparam_all` (when the arguments alone determined no type parameter at all)
- the "preserve the receiver's collection" shortcut for `map` (the element type is the
  function's return type)

The `scala.FunctionN` **class** itself is out of scope for this view. It is already treated as a
function everywhere, and rewriting it structurally here would make both overloads of `map`
applicable at once.

**6. `WithFilter` had no type constructor.** 2.13 has `class WithFilter[+A, +CC[_]]` with
`map[B](f: A => B): CC[B]`. The prelude put an **already applied** collection (`List[A]`) into
`CC` and made `map: CC`, so `for (x <- xs if p) yield x.toString` stayed a `List[Int]`
(jar mode only; under the private runtime `withFilter` returns the receiver itself). `CC` is
now a type parameter of kind 1, and `map` / `flatMap` have their own `B` and return
`Type::Applied { ctor: CC, args: [B] }`.

**7. Value definitions in a for comprehension were counted as generators.**

```scala
for { m <- ms if m > 0; q = f(m) } yield q
```

A `q = e` becomes a `val` in the lambda body, so the generator before it is **still the
innermost** and takes `map`. Because we counted by enumerator position, it became `flatMap`,
and the function was shaped to return an element rather than a collection. The shape where a
**guard** follows a value definition (nsc pairs them into a tuple and filters the stream) cannot
be expressed by this desugaring, so it is **reported as a diagnostic**
(`tests/fixtures/mism6_forval_bad.scala`).

**8. The collection hierarchy lacked `scala.collection.IndexedSeq` and the mutable backbone.**
`ArrayBuffer` was nowhere an `IndexedSeq`, so slick's
`def and(ns: scala.collection.IndexedSeq[Node])` could not accept the `ArrayBuffer` it had built
itself. `collection.IndexedSeq` / `mutable.Seq` / `mutable.IndexedSeq` / `mutable.Buffer` were
added to `prelude_hier.rs`, and `ArrayBuffer` and `ListBuffer` were wired into them.

**9. The `apply` of `Success` / `Failure` had no type parameter.** `apply` returned a raw
`Success` / `Failure`, so `def a[R](…): Try[R] = Success(f)` gave
`found: Success required: Try[R]`. The `T` of `Failure.apply[T]` appears in no parameter, so
only the expected type (or `Nothing`; harmless, since `Try` is covariant) can determine it.

slick: `errors 537 → 526`, `type mismatch 90 → 83`, `files_with_errors` unchanged at 80. The
set of files that report errors is unchanged.
### lub of captured parameters and invariant arguments (`type mismatch`, slice 7)

The `agent/mismatch7` slice. Fixtures are `tests/fixtures/mism7*.scala`, tests are
`crates/cli/tests/mismatch7.rs`. Eight root causes fixed.

**1. A method's parameter was being as-seen-from'd through an anonymous class**.

```scala
trait It[T] { self =>
  def next(): T
  def map[B](f: T => B): It[B] = new It[B] { def next(): B = f(self.next()) }
}
```

`bind_found` said: if the owner of the symbol found differs from `this_class`,
apply as-seen-from. But the owner of `f` is **the method `map`**, not a class.
Inside the anonymous class `this_class` is that anonymous class (whose parent is
`It[B]`), so `T := B` was substituted into `f: T => B`, yielding `(B) => B`.
**Only class members are read through a prefix**: as-seen-from now applies only
when the owner is `Class` / `ModuleClass` / `Module`.
Passing the anonymous class's own `this.next()` is still a mismatch, exactly as
in nsc (`tests/fixtures/mism7_capture_bad.scala`).

**2. Template rules were being applied to compound *types***. `compound_to_type`
reported "illegal inheritance" whenever there were two or more class parents
with no most specific one among them. nsc's
`typedCompoundTypeTree` has no such check. `def f(x: A with B)` **is accepted
even when A and B are unrelated classes** (there simply is no value of that
type). slick's `Query[B, BU, C] & TableQuery[B]` (one is a subclass of the
other) failed on this, and all three of `Executable`'s implicits became
`<notype>` outright.

In its place we added the rule nsc really does have: `validateParentClasses`,
**every parent after the first in a template must be a trait**
(`check_mixin_parents`). `class C extends A with B` gives
`class B needs to be a trait to be mixed in`, which is scalac 2.13.16's
message verbatim. The existing `compound_bad.scala` pinned this
"well-formed as a type" shape, so we replaced it with member resolution on a
compound type (a name neither parent declares) and split the template side out
into `mism7_mixin_bad.scala`.

**3. Eta expansion was solving type parameters from the *result* of the expected
function type**.

```scala
xs.map(identity)   // found: CA[Any]  required: CA[T]
```

A function's parameters are contravariant and its result covariant, so
`A => A <: T => ?U` means `T <: A` and `A <: ?U`. nsc solves `A` from the
**parameter** side and uses the result only as an upper bound. Taking both at
once, the result expected by the still-being-inferred `map` is `Any`, so the lub
swallowed `T`. We solve from the parameters and consult the result only for what
was left undetermined there (parameters that appear only in the result)
(`Typer::solve_eta_tparams`). The explicit `f _` form (`type_eta`) now goes
through the same path, so `val h: String => String = identity _` compiles too.

**4. An abstract type's *lower* bound was not used on the right-hand side of
`<:`**.

```scala
def f[E, O >: E](x: E): O = x
```

When the right-hand side is an abstract type, nsc tries `tp1 <:< tr2.lo`. We had
only the upper-bound rule (`bound_hi`, keyed on `(TypeParam(id), b)`) and **not a
single** rule that looked at the lower bound. It goes at the head of
`is_sub_type` — before the other branches, since each of them either matches on
`a` alone or only asks "is it the same parameter". This makes slick's shape of
passing a `ShapedValue[_ <: E, U]` where `ShapedValue[_ <: O, U]` is expected
(5 occurrences in `Query.scala`) compile. The reverse direction
(`def wrong[E, O >: E](x: O): E`) is a mismatch, exactly as in nsc
(`tests/fixtures/mism7_lobound_bad.scala`).

**5. 2.13's `SeqOps` declares `indexWhere` twice**.

```scala
def indexWhere(p: A => Boolean, from: Int): Int
def indexWhere(p: A => Boolean): Int
```

The pickle supply has a rule that only one overload taking a function is
admitted (a lambda's parameter types can only be inferred from a single expected
type, so with two function overloads of the same name `xs.segmentLength(_ < 3)`
becomes unsolvable). Only the first in linearization order — the two-argument
version — was admitted, and `xs.indexWhere(p)` was an arity error. **The number
of arguments is known before the lambda is typed**, so the rule became one per
name **and arity**. `indexOf` / `lastIndexWhere` / `segmentLength` have the same
shape.

**6. Re-pointing a module at its `apply` read a signature nobody had
completed**. When you write `Module[T1, T2]` and omit the `.apply`, the
`TreeKind::TypeApply` branch re-points the symbol at the `apply` of the module's
companion. But this path **does not go through a select**, so the
`complete_lazy_sig` that `bind_found` performs never runs. An `apply` named
before its own definition, with an inferred result type, stayed `<notype>`
(slick's `Executable.queryIsExecutable = StreamingExecutable[…]`;
`object StreamingExecutable` is 25 lines below it). We now complete the symbol
we re-point to.

**7. An argument left with only its implicit clause was being filled in after it
had already constrained the call**.

```scala
def one[A2](a2: A2): Int = 0
one(kvs.toMap)                 // found: Map[String, Int]  required: Map[K, V]
(1, kvs.toMap)                 // same
```

Typed with no expected type, `toMap[K, V](implicit ev: A <:< (K, V))` stays as
the method type `(A <:< (K, V))Map[K, V]`. `A2` was determined from that
**still-unresolved** result, and only afterwards did the witness fix `K`/`V` to
`String`/`Int`, so the type the argument had to conform to was left stranded at
`Map[K, V]`. nsc adapts the argument and only then constrains the call: inside
the argument loop we now fill in the implicit clause **before** substituting the
parameter's undetermined variables. The existing path that solves the receiver's
undetermined variables from the arguments then carries the filled-in result to
the parameters, the result and the receiver.

**8. The lub of invariant type arguments came out as a type that accepts neither
side**.

```scala
Seq(new Inv[Boolean], new Inv[Int])
```

For the same class with differing arguments we joined argument by argument, but
for an **invariant** parameter neither `Inv[Boolean]` nor `Inv[Int]` is an
`Inv[Any]`. nsc's lub builds an existential here
(`Inv[_ >: Int with Boolean <: AnyVal]`). We now build an upper-bounded wildcard
as well.

At the same time, **a varargs call is no longer re-wrapped into a tuple**
(`callee_takes_repeated`). nsc's `tryTupleApply` runs only when the number of
formals and the number of arguments disagree, and a repeated parameter is
expanded out to the argument count before the comparison, so the two always
agree. This is why the `Seq(a, b)` above turned into
`Seq[(Inv[Boolean], Inv[Int])]` the moment it stopped being applicable.

slick: `errors 518 → 495`, `type mismatch 96 → 84`, `files_with_errors`
80 → 77. No file gained errors.

### Expected types, varargs, dependent method types (`type mismatch`, slice 8)

The `agent/mismatch8` slice. Fixtures are `tests/fixtures/mism8*.scala`, tests are
`crates/cli/tests/mismatch8.rs`. Seven root causes fixed. The details are in the
"Type aliases", "Inference of method type parameters", "`-Xsource:3`" and
"Overload candidate sets" sections above.

1. **The expected type was not dealiased when it was an alias**. `collect_expected`
   matches `Map[K, V]` against `Type$.Scope` structurally, so
   `val s: Type.Scope = Map.empty` came out as `Map[Nothing, Nothing]`.
2. **Empty varargs stayed "unresolved"**. `List()` / `Seq()` / `Map()` have
   nothing from which to determine the element type — that is **no constraint**,
   which does not license holding on to the callee's type parameter.
3. **`xs: _*` was stripped on one side only**. `def mk[A](xs: A*)` solved to
   `A = Int*`, and `mk(xs: _*)` became `List[Int*]`.
4. **The `-Xsource:3` splat `f(xs*)` was unsupported** (it parsed as a postfix
   operator: `value * is not a member of Seq[…]`). Three occurrences of
   `Map(elems*)` in slick.
5. **nsc's `protoTypeArgs` was missing**. Each component of a tuple should be
   typed against the corresponding component of the expected type; without that,
   an invariant `Map` freezes on the key type from the argument side.
6. **Dependent method types** (`def get[P <: Phase](p: P): Option[p.State]`).
   `Type::TypeMember` carries no prefix, so we look for the parameter that could
   have served as the prefix — only when the bounds determine it uniquely — and
   substitute the same-named member of that argument. The four
   `if(…getOrElse(true))` that had fallen to `Any`, and their cascades, are gone.
7. **`private[p]` was being resolved in the use site's scope**. `private[util]`
   hit `scala.util`, so even references from inside `slick.util` were rejected.
   Along with it we added nsc's `nonLocalMember` (a `private[this]` member is not
   a member of any prefix other than `this`) on the selection side too.

slick: `errors 411 → 378`, `type mismatch 58 → 49`, `files_with_errors`
72 → 67. No file gained errors.

Understood down to the root cause but **not fixed** in this slice:

- `mutable.ArrayBuilder[T]` / `StringBuilder` / `ListBuffer` have no
  `Builder[…]` / `Growable[…]` base type (`x.result()` is `Any`). This is the
  mutable-collection hierarchy, so it belongs to `agent/mutcoll`.
- The 13 `found: F[Any] required: F[R]` in `BasicBackend.scala` /
  `ConcurrencyControl.scala` were written up as a cascade from a **real error**
  in the lambda body, but **that reading was wrong**. There is no error inside
  the lambda; `collect_expected` simply had no arm matching a higher-kinded
  application (`Applied`) against the expected type. Fixed in slice 9
  (the "Higher-kinded expected types…" section below).

### Higher-kinded expected types, overloads on sorted collections, `copy` inside a class (`type mismatch`, slice 9)

The `agent/mismatch9` slice. Fixtures are `tests/fixtures/mism9_*.scala`, tests are
`crates/cli/tests/mismatch9.rs`. Five root causes fixed.

1. **A higher-kinded application could not solve type parameters from the
   expected type**. When the `F` of
   `def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]` is an abstract type
   constructor (`F[_]`), the result type is `Type::Applied`, not `Type::Class`.
   `collect_expected` (nsc's `instantiateExpecting`) had arms only for `Class` /
   `Tuple` / `Function` / `Array`, and none matching an `Applied` against an
   `Applied`. `B` was determined neither by the expected type `F[String]` nor by
   the arguments, so every cats-style `F.flatMap(fa) { … }` came out as `F[Any]`.
   Argument positions of a type constructor carry no variance annotation, so we
   treat them as **invariant**. For the shape where the expected type has already
   resolved to a concrete class (`F[B]` against `List[String]`) we match the
   constructors in their **unapplied** form, so that `F` itself cannot be solved
   to `List[String]`.

   These are exactly the 13 that slice 8 recorded as a "cascade from a **real
   error** in the lambda body". **That record was wrong.** There is no error
   inside the lambda; it is a pure inference gap that minimizes to six lines
   (`mism9_hk_result_comes_from_the_expected_type` in
   `crates/cli/tests/mismatch9.rs`). Real scalac 2.13.16 accepts it.

2. **Two candidates differing only in their implicit argument list both
   survived**. 2.13 declares
   `map[B](f)(implicit ord: Ordering[B]): CC[B]` on `SortedSetOps` and
   `map[B](f): CC[B]` on `IterableOps`. nsc's `isAsSpecific` **passes straight
   through** an implicit clause (`case mt: MethodType if mt.isImplicit =>
   isAsSpecific(restpe, …)`), so the two are equally specific, and the only thing
   that settles it is `relativeWeight`'s `isInProperSubClassOf` — **the subclass
   relation between the declaring classes**. But `pickle_supply` lowers inherited
   members onto the receiver's class, so that owner is lost (both `collect`s on
   `TreeSet` have `owner=scala/collection/immutable/TreeSet`).

   Three places were fixed.
   - On the supply side, the key for "only one candidate per argument list" is
     now built from **the explicit parameters only**. The one that comes first in
     linearization order — the more derived declaration, the one taking the
     `Ordering` witness — is the one kept.
   - That key is claimed only **after the descriptor has been looked up**.
     `TreeMap.collect(pf)` cannot be looked up uniquely in the classfile, and
     taking the key and then bailing out stole the slot for
     `collect(pf)(Ordering)` as well.
   - We added nsc's `isInProperSubClassOf` on the typechecking side too (for
     ordinary inheritance, where members have not been lowered onto the same
     owner, this is what decides).
   - Codegen's `scala.collection` quick-reference table bakes in the
     `IterableOps` shape `map:(Lscala/Function1;)…`, so members that **come from
     the pickle and carry an implicit clause** bypass that table and are called
     with the descriptor the pickle recorded. Without this, `TreeSet.map(f)`
     pushes two arguments and emits a one-argument call, giving
     `IncompatibleClassChangeError` (having got rid of the `ambiguous overload`,
     it had turned into **silent miscompilation**).

   `TreeSet.map` / `flatMap` / `collect` and `TreeMap.map` / `flatMap` are
   accepted, and even with the static type narrowed to `TreeSet[Int]` a `TreeSet`
   comes back at runtime.

3. **A `copy(…)` written inside the class**. `p.copy(y = 3)` was rewritten into a
   constructor call so that the type parameters get re-inferred, but the
   receiver-less form `copy(from = f2, …)` (`TreeKind::Ident`) was not a rewrite
   target and kept the synthetic member's argument types — that is, **the class's
   own** type parameters. slick's
   `case class Comprehension[+Fetch <: Option[Node]](…, fetch: Fetch =
   None, …)` therefore cannot be rebuilt with a different `Fetch` and gives
   `found: Option[Node] required: Fetch`. nsc synthesizes
   `copy[Fetch <: Option[Node]](…): Comprehension[Fetch]`, so the type parameters
   are re-solved at every call. We rewrite only when the name really resolves to
   this class's own synthetic `copy` (a local `def copy`, an import, or an
   inherited one stays an ordinary call).

4. **`foreach` was not polymorphic in the function's result**. 2.13 has
   `IterableOnceOps.foreach[U](f: A => U): Unit` (`javap -s`:
   `<U:Ljava/lang/Object;>(Lscala/Function1<-TA;+TU;>;)V`). The prelude wrote
   `A => Unit`, and `Function1[Int, R]` does not conform to that. A lambda
   **literal** was accepted because its body is discarded, but a **function
   value** such as `def foreach[R](f: Int => R): Unit = r.foreach(f)` was not.
   There are more than twenty declarations, so, to state the rule in one place,
   `crates/typer/src/prelude_mism9.rs` polymorphizes on the spot exactly the
   shape the prelude wrote (a `foreach` with no type parameters, one parameter,
   taking `A => Unit` and returning `Unit`). `U` erases to `Object` and the
   argument is a `Function1` either way, so the descriptor does not change.

5. **A tree that got no type was reported twice**. The final `type mismatch` in
   `adapt` also fired when `found` was `<notype>` — that is, when **the typer
   could not give that tree a type at all**. The cause is always reported at that
   tree, so this is a repeat (nsc's `ErrorType` absorbs it the same way). Other
   arms of the same function (overloads, constructors) were already built to stay
   silent about an operand that has already failed, so we lined this up with
   them.

slick: `errors 327 → 308`, `type mismatch 44 → 26`, `files_with_errors` 64
(unchanged). **Not a single new kind of error appeared, and no file newly became
an error.**

Understood down to the root cause but not fixed in this slice, plus the ones that
**could not be minimized**:

- `K2` in `TreeMap.collect { case (k, v) => … }` is `Any` (it goes looking for
  `Ordering[Any]`). `TreeSet.collect { case x => x }` (a single type parameter)
  is accepted. Only the shape combining pair destructuring with an implicit
  clause remains. **Fixed in slice 10** (item 3 of "The two class-header
  passes…" below). Note that what was written here at the time —
  "`tm.collect(pf)` (a value with a type annotation) is accepted" — **was
  wrong**: typechecking passed, but at runtime a `List` came back (item 4 of that
  same section).
- Two `found: DDL required: SchemaDescriptionDef` in `MemoryProfile`.
  `class DDL extends SchemaDescriptionDef` and
  `type SchemaDescription = SchemaDescriptionDef` point at the same trait, yet do
  not conform. Even adding a diamond of inheritance, a self type and a separate
  file, **no minimal reproduction could be built** (nothing was added to
  `tests/fixtures`).
- `found: ActionListener[F] required: ActionListener[F]` (identical rendering,
  different symbols) in `HeapBackend` / `DistributedBackend`. It is the shape
  where `override val al: AL[F] = AL.noop[F]` is written as a constructor
  default, and here too **no minimal reproduction could be built**.
  -> **Minimized and fixed in slice 10** (one line,
  `class HkBox[F[_]](val cell: Cell[F] = Cell.empty[F])`; the `F` on the `found`
  side was not even a different symbol, it was **an unresolved name**).
- Two `found: TypedType[Option[Option[Any]]] required: TypedType[Option[Any]]` in
  `OptionMapper.scala` (brought in by `agent/buildfrom`). Copying the
  `trait OptionTypedType[T] extends TypedType[Option[T]]` hierarchy does not
  reproduce it.
- The three `BP` / `P` in `ExtensionMethods.scala` are a cascade from the
  `No matching Shape found` just before them (implicit search for slick's
  `Shape`).
- `mutable.ArrayBuilder` has no `Builder[E, Array[E]]` base type (the
  mutable-collection-hierarchy gap carried over from slice 8). In slice 10 this
  looked like exactly the "no parents on stubs" restriction (`ArrayBuilder` /
  `Iterator.GroupedIterator` are stubs **whose members have never once been asked
  for**, so they have no parent chain). -> **That reading was wrong.**
  Re-checking in slice 11 showed that `GroupedIterator` had its parent
  (`AbstractIterator[Seq[B]]`) as soon as `withPartial` was asked for, and that
  the cause was **capture in the linearization substitution**. `ArrayBuilder` too
  had its parent, from the classfile, and the cause was that **that parent's
  arguments had been erased** (see "Type-parameter capture in inherited
  members…" below).

### The two class-header passes and `collect` on sorted maps (`type mismatch`, slice 10)

The `agent/mismatch10` slice. Fixtures are `tests/fixtures/mism10_*.scala`, tests
are `crates/cli/tests/mismatch10.rs`. Four root causes fixed. Two of them were
also silent miscompilations: **typechecking passed and then something else came
back at runtime, or a `VerifyError` resulted**.

1. **The actual arguments of a parent constructor were being reported along with
   the diagnostics of the signature pass**. A parent's actual arguments are just
   expressions. `typecheck_units` runs a "signatures only" pass once over all
   units before typing bodies, and in that first half the members of *later
   files* do not have types yet. slick's

   ```scala
   case class ColumnOrdered[T](column: Rep[T], ord: Ordering)
     extends Ordered(Vector((column.toNode, ord)))
   ```

   has `Rep.scala` later on the command line, so in the signature pass `toNode`
   is not yet a member, the pair became `(?T1, Ordering)`, and it emitted
   `found: Vector[Tuple2[T1, Ordering]] required: IndexedSeq[(Node, Ordering)]`.
   The body pass types **the very same tree again** and correctly gets
   `(Node, Ordering)`. By the same reasoning that discards header-pass
   diagnostics, we discard signature-pass diagnostics for parent-constructor
   applications. A parent argument that really is wrong is reported as-is by the
   pass in which all the signatures are in place
   (`mism10_wrong_parent_argument_is_rejected`). **It also happens within one
   file, by declaration order** (`mism10_parent_argument_sees_a_later_member`).

2. **A primary constructor's default argument could not look up the class's type
   parameters by name**. A primary constructor has no type parameters of its own
   (`A` belongs to *the class*). On top of that, a constructor default has no
   `name$default$n` getter (at the point of `new Foo(1)` there is no receiver).
   So the tree the namer had saved was being typed as-is **in the call site's
   scope**, where there is no binding for `A`.

   ```scala
   class Box[A](val one: List[A] = List.empty[A])   // found: List[A]  required: List[A]
   class HkBox[F[_]](val cell: Cell[F] = Cell.empty[F])
   ```

   The `A` on the `found` side was **an unresolved name** (`Type::Named`). Before
   typing the body of a default, we now bind by name the type parameters in which
   that parameter's type is written. This is what slick's
   `found: ActionListener[F] required: ActionListener[F]` (identical rendering,
   different symbols) in `HeapBackend` / `DistributedBackend` really was — the
   one slice 9 recorded as "no minimal reproduction could be built". Defaults on
   ordinary methods are unaffected, since those methods have type parameters of
   their own (`mism10_method_default_still_works`).

3. **When an undetermined type variable sat inside a pair, the body of a partial
   function literal could not determine it**. A callee type variable the call has
   not solved arrives at the argument position as its *declared upper bound*
   (`open_to_bounds`). `SortedMapOps.collect[K2, V2](pf: PartialFunction
   [(K, V), (K2, V2)])(implicit Ordering[K2])` arrives at the literal as
   `PartialFunction[(Int, String), (Any, Any)]`. A **bare** type variable was
   already treated as "saying nothing" and left to the body to determine, but a
   type variable **inside a pair** was not, so the `case` body was typed as
   `(Any, Any)` and went looking for `Ordering[Any]`. A *tuple* made up entirely
   of type variables opened to their upper bounds is now treated as "saying
   nothing" too. Tuple elements are always references, so there is no risk of
   dropping boxing that the expected type `Any` was forcing.

4. **Members supplied from the pickle landed on an ancestor rather than the
   receiver, and the outcome depended on order**. A library member is read from
   the pickle at the point it becomes necessary and lands on **the class that
   declares it**. Once it lands on an ancestor, every later inheritance lookup
   hits it, so the derived class's own overloads are never asked for again.

   ```scala
   val plain = Map(1 -> "a")
   println(plain.collect { case (k, v) => (k, v) })      // MapOps.collect lands on Map here
   val pf: PartialFunction[(Int, String), (Int, Int)] = { case (k, v) => (k, v.length) }
   TreeMap(1 -> "a").collect(pf)                          // -> List((100,1)) was returned
   ```

   `TreeMap.collect` resolved to `MapOps.collect(pf)`, and the call was emitted as
   `IterableOps.collect`. Its default implementation builds through
   `iterableFactory`, so **a `List` comes back**. No diagnostic appeared
   anywhere, and on top of that the result changed depending on whether a
   `Map.collect` appeared earlier in the same file. When only an inherited member
   is found, we now ask the pickle a second time — **only when the receiver's
   classfile declares a method of the same name with a different arity** — and
   match the two up (`TreeMap.collect(PartialFunction, Ordering)` against
   `MapOps.collect(PartialFunction)`). A plain override with the same descriptor
   (`List.length` against `Seq.length`) is settled by virtual dispatch, so we do
   not ask — the prelude types `aSet.toSeq` as `List` while the `toSeq` actually
   called returns `Seq`, so emitting `invokevirtual List.length` on that value
   would give a `VerifyError`.

slick: `errors 257 → 241`, `type mismatch 25 → 22`, `files_with_errors 63 → 61`.
**Not a single new kind of error appeared, and no file newly became an error.**
(The 327 / 44 / 64 recorded by slice 9 correspond to a baseline of 257 / 25 / 63
on today's `main`, after `agent/tail1`, `agent/quasi` and others landed.)

Known but not fixed in this slice:

- The family of consequences of the "**no parents on stubs**" restriction:
  `mutable.ArrayBuilder` has no `Builder[E, Array[E]]` base type,
  `Iterator.GroupedIterator[B]`'s element type comes out as `B` instead of
  `Seq[B]`, and so on (see "What is still not possible" above).
  `xs.iterator.grouped(2).map { case Seq(i, t) => (i, t) }` emits a lambda with
  the wrong element type, so it is also a silent miscompilation ending in
  `VerifyError`. -> **Fixed in slice 11.** The cause is not this restriction
  (next section).
- Two `found: DDL required: SchemaDescriptionDef` in `MemoryProfile`. We managed
  to write the shape where the abstract type member
  `type SchemaDescription <: SchemaDescriptionDef` is pinned to
  `= SchemaDescriptionDef` in a subclass, but it does not produce slick's symptom
  (it turns into a different, unresolved `Basic.SchemaDescription`), so it is
  still not minimized.
- Two `TypedType[Option[Option[Any]]]` in `OptionMapper`, three `BP` / `P` in
  `ExtensionMethods`, three in `Query.scala`, two `E with Effect` in
  `JdbcActionComponent`, and the eta expansion of `BigDecimal.apply` at
  `Type.scala:388` (**it does not reproduce in a single file**; it does not
  reproduce even with `java.math.MathContext` in the symbol table, so it depends
  on several files).

### Type-parameter capture in inherited members, and erased parents (`type mismatch`, slice 11)

The `agent/mismatch11` slice. Fixtures are `tests/fixtures/mism11_*.scala`,
tests are `crates/cli/tests/mismatch11.rs`. Three root causes fixed. Two of them
**passed typechecking and then destructured something else at runtime, or refused
a call that should have been legal**; and the two items slice 10 recorded as the
"**no parents on stubs** restriction" turned out to be **neither of them that
restriction** (found by re-checking the inherited diagnostics).

1. **The pickle's linearization substitution was captured by the method's own
   type parameters**. An inherited member is rewritten into the vocabulary of the
   class that was asked, by substituting at each hop the arguments the child
   passed to its parent (`SigCache::lookup`).
   For `Iterator.GroupedIterator[B] extends AbstractIterator[Seq[B]]` that
   substitution is `A := Seq[B]`, and the member it lands on,
   `Iterator.map[B](f: A => B): Iterator[B]`, binds **a `B` of its own**. Because
   the substitution went by name, *the class's* `B` fell under *the method's*
   binder and `map` collapsed into a single type that takes `B` rather than
   `Seq[B]`. `apply_subst` is now capture-avoiding: it renames only those type
   parameters that bind a name occurring free on the **value side** of the
   substitution (`avoid_capture` in `crates/pickle/src/sym.rs`).

2. **The rule "a collection's element type is the receiver's first type
   argument" was overriding the argument type the declaration states
   outright**. What `grouped(n)` returns is a `GroupedIterator[B]`, whose
   elements are `Seq[B]`. A guess must not beat a declaration, so the override
   now applies only when **there is a single argument and its type is not yet
   determined**. As a bonus this also fixes the collapsing of a **two-argument**
   function such as `LazyZip2[A, B, C].map(f: (A, B) => R)` into one argument
   (`xs.lazyZip(ys).map((a, b) => …)` was giving `found: (String, Int) => String
   required: (String) => Any`).
   With 1 and 2 together, `clauses.iterator.grouped(2).map { case Seq(i, t) => (i, t) }`
   (slick `Node.scala:724`) is accepted. It **had been emitting a lambda with the
   wrong element type**, so it was also a silent miscompilation ending in
   `VerifyError`.

3. **Pickle type parameters were not attached to `scala.` placeholders**.
   `find_or_stub_java_class` enters every name mentioned in a classfile's parent
   list as an empty symbol. `give_stub_its_kinds` is the part that attaches type
   parameters to those, but it uniformly refused names beginning with `scala/`.
   The reason for refusing is "do not rebuild symbols **the prelude
   constructed**", so the line is `prelude_end`, not the `scala.` package.
   `scala/collection/mutable/ReusableBuilder`, entered from `ArrayBuilder`'s
   parent list, is exactly such a case: `ReusableBuilder[T, Array[T]]` became
   "two arguments but zero symbols", and `ArrayBuilder` got no parent at all.
   Furthermore, a classfile's generic signature can only say
   `ArrayBuilder<T> implements ReusableBuilder<T, Object>`. `To` is invariant, so
   that does not give `Builder[E, Array[E]]`. When **a parent pointing at the same
   class is already present with different arguments**, we now **refine** it from
   the pickle side (scalac's own record), never touching prelude classes. This
   lets `mutable.ArrayBuilder.make[E]` be returned as
   `mutable.Builder[E, Array[E]]` (slick `Type.scala:203`).

4. **An undetermined *type constructor* was arriving at the argument's expected
   type as an upper bound**. `Any` is not an inhabitant of a constructor's
   **kind**. slick's `flatMap[F, T, D[_]](f: E => Query[F, T, D])` arrived at the
   lambda as `Query[F, T, Any]`, and the body's `Query[G, T, Seq]` became
   `found: Query[G, T, Seq] required: Query[G, T, Any]`
   (`Query.scala:37`). `open_to_bounds` now opens with a wildcard when the type
   parameter itself has type parameters (i.e. is a constructor). It just writes
   "some type not yet determined" in a form `is_sub_type` already understands.

slick: `errors 237 → 234`, `type mismatch 20 → 17`, `files_with_errors 60`
(unchanged). `tests/slick_subset.sh` is unchanged at `verified=204 failed=0`.
**Not a single new kind of error appeared, and no file newly became an error.**

Known but not fixed in this slice:

- `LazyZip2.map` **is now supplied**, as a result of item 3 above making
  `BuildFrom[C1, B, C]` expressible, but only implicit search can determine `C`,
  and our search looks for a type we already hold, so it cannot solve variables
  inside the type being searched for. Even
  `implicitly[BuildFrom[Seq[String], String, Seq[String]]]` (fully applied) is
  not found. As a result, slick's five occurrences went from one
  `value map is not a member of LazyZip2[…]` to one
  `no implicit: could not find implicit value of type BuildFrom[…]` plus one
  cascade off the unresolved `C` (all of them existing kinds; no file newly
  became an error). Really finding the `BuildFrom` implicit needs an implicit
  search that can match
  `buildFromIterableOps[CC[X] <: Iterable[X] with IterableOps[X, CC, _], A0, A]`
  higher-kinded. We also tried the opposite direction, **cutting off the supply**
  ("do not supply members whose type parameters are determined only by an
  implicit clause"), but that bit far too hard — `errors 235 → 309` — so it is
  not in.
- Of the remaining 17 `type mismatch`, the two `DDL /
  SchemaDescriptionDef` in `MemoryProfile`, the three `BP` / `P` in
  `ExtensionMethods` (a cascade from `No matching Shape found`), the two
  `E with Effect` in `JdbcActionComponent`, the remaining two in `Query.scala`,
  and the eta expansion of `BigDecimal.apply` at `Type.scala:388` are all carried
  over from slices 9 and 10 and do not reproduce in a single file.
  The two `found: Product required: Option[Option[Any]]` in `JdbcModelBuilder` /
  `SQLiteProfile` look like the lub of `if (v == "NULL") None else Some(…)`
  failing to come out as `Option`, but **writing that shape on its own does not
  reproduce it** (they are a cascade from other errors in the same file).
- `ConcurrencyControl.scala:202` only changed from `found: State[Any]` to
  `found: State[_]` with the change in item 4, and is still an error (that one is
  on the cats `Ref.of[F, State[F]]` side).

### Type-constructor bounds, an `apply` of its own, implicits inherited by a companion (`type mismatch`, slice 12)

The `agent/mismatch12` slice. Fixtures are `tests/fixtures/mism12_*.scala` and
`tests/multi/mism12_*.scala`, tests are `crates/cli/tests/mismatch12.rs`.
Six root causes fixed. Two of them **passed typechecking and then selected a
different member or a different type**. Among the inherited diagnostics, "the
`(Double)` overload is not supplied" (slice 11) was right, and "deriving the
`Shape` implicit is the main event" (slice 11) was right too, but the real cause
was not the derivation: it was that **implicits the companion inherited were
never candidates in the first place**.

1. **The upper bound of a type-constructor parameter was not instantiated with
   the arguments of the application**. `M[A]` (where `M[+X] <: IterableOnce[X]`)
   is `IterableOnce[A]`. The bound is written in terms of the constructor's own
   parameters, so it means nothing until those are substituted.
   `widen_type_param` only widened a **bare** `M`, so in slick's
   `DBIOAction.traverse[A, B, M[+X] <: IterableOnce[X]]` the `in.iterator`
   returned `IterableOnce`'s own `A`, and every use of an element gave
   `found: A required: A` (**different symbols with identical rendering**)
   (`DBIOAction.scala:349`).

2. **A case class's companion `apply` was handed the class's type parameters
   as-is** (the class's type parameters doubling as the method's). A single
   symbol then meant both "already fixed here" and "to be inferred at this call",
   and for **a call from inside the class** the substitution `U := U` left the
   argument type still containing the callee's type parameter — read as
   "undetermined" — so the argument was checked against the **upper bound**
   (`found: Bx[U] required: Bx[Any]`). `fresh_method_tparams` now gives `apply`
   type parameters of its own (same names, kinds and bounds; no variance
   annotations). slick's `ShapedValue.packedValue` (`ShapedValue.scala:16`) is
   accepted.

3. **`scala.math.BigDecimal`'s companion had only 3 of its 17 `apply`s**.
   Hand-written prelude members refuse the copy from the pickle
   (`agent/setapply`), so the missing ones **simply do not exist**.
   `new ScalaNumericType[BigDecimal](BigDecimal.apply)` (`Type.scala:388`)
   eta-expands at `Double => BigDecimal`, so there was nothing to select. We
   wrote all 17 that `javap` prints into
   `crates/typer/src/prelude_mism12.rs` (`library_abi` only; the private runtime
   does not emit `scala/math/BigDecimal$`, so a diagnostic is reported in non-jar
   mode).

4. **Implicits a companion *inherited* were not candidates**. What SLS 7.2 talks
   about is the companion **object**, and an object's members include inherited
   ones. slick writes every one of its `Shape` instances in
   `trait RepShapeImplicits` / `ConstColumnShapeImplicits` /
   `TupleShapeImplicits` and declares `object Shape extends
   ConstColumnShapeImplicits with …`, so **not one of them was a candidate**.
   `companion_implicits_of_class` now walks up to the parents. Emitting an
   inherited one under its bare name produces code that pushes `this` and casts
   to the declaring trait (`Main$ cannot be cast to ConstShapes`), so we make
   **the object we came through the receiver** (`implicit_via_module`, handled the
   same way as the existing `wildcard_module_for` for wildcard imports).

5. **Implicit unification could not handle `_` or contravariant positions**. A
   `_` in the type being searched for means "we are not asking about that", so it
   matches anything (the `?` of
   `packedValue[R](implicit ev: Shape[? <: FlatShapeLevel, T, ?, R])` was being
   matched structurally against a candidate's `U` and pronounced a mismatch).
   Contravariant parameters go the other way round: **the type being searched for
   is the subtype** (`constColumnShape: Shape[L, ConstColumn[T], T, ConstColumn[T]]`
   answers `Shape[FlatShapeLevel, LiteralColumn[Boolean], ?, ?BP]`).
   With 4 and 5, the two `fold` (`BP`) in `ExtensionMethods` and
   `Query.scala:290` compile, and the three
   `value toNode/zip is not a member of (…)…` that cascaded from them disappear
   as well.

6. **A lazily resolved type alias was pinned to the scope of the first round of
   the header pass**. `refresh_alias_sigs` makes a pending alias remember "that
   template's scope", but **only the first time**. The header pass is designed to
   repeat until the parent chains stop changing, and the inherited members of a
   class **whose grandparent lives in a later file** are only visible from the
   second round on. slick's `trait MemoryProfile extends RelationalProfile`
   (`slick/memory/` comes before `slick/relational/`) writes
   `type SchemaDescription = SchemaDescriptionDef`, where `SchemaDescriptionDef`
   is a trait nested in `BasicProfile`, and `MemoryProfile.scala` does not import
   that name. A nested class's constructor parameter
   (`class MemorySchemaActionExtensionMethodsImpl(schema: SchemaDescription)`)
   completes the alias during the header pass, so the right-hand side stayed an
   **unresolved `Type::Named`** and `new DDL(…)` gave
   `found: DDL required: SchemaDescriptionDef` (**two different things both
   rendered as `SchemaDescriptionDef`**). We now use the scope of the **last**
   round (every round's scope is the template's own, so a later one is always
   more complete). These are the two that slices 9 through 11 failed to minimize
   three times running; they reproduce with the four files of
   `tests/multi/mism12_*.scala`.

slick: `errors 223 → 209`, `type mismatch 17 → 9`, `files_with_errors 60`
(unchanged). `tests/slick_subset.sh` is unchanged at `verified=204 failed=0`.
**Not a single new kind of error appeared, and no file newly became an error.**

Known but not fixed in this slice:

- In `a ++ b`, when `++` is the declaration on `SchemaDescriptionDef` (whose
  parameter is the abstract type member `SchemaDescription`), the parameter type
  of `++` as seen from `MemoryProfile` stays `BasicProfile.SchemaDescription`,
  giving `no matching overload for (BasicProfile.SchemaDescription)…`. This is a
  different gap from item 6 (**as-seen-from for abstract type members**), so it
  is excluded from `tests/multi/mism12_*.scala`.
- The remaining nine `type mismatch`:
  `found: <overload String | <error>>` at `Node.scala:636`,
  `ConcurrencyControl.scala:202`, the two `E with Effect` in
  `JdbcActionComponent`, the two `found: Product required:
  Option[Option[Any]]` in `JdbcModelBuilder` / `SQLiteProfile`,
  `ExtensionMethods.scala:210` (`flatten`'s `P <:< Rep[Option[QO]]`),
  `Query.scala:153`, and `RelationalProfile.scala:72`.
- The higher-kinded `BuildFrom` matching for `LazyZip2.map` that slice 11
  recorded is unchanged (items 4 and 5 do not reach it).
### Views brought in by `import <value>._` (`agent/tail2`)

The fixtures are `tests/fixtures/t2_*.scala`, the test is
`crates/cli/tests/tail2.rs`. What slick's `MySQLProfile` /
`JdbcStatementBuilderComponent` write,

```scala
import seq.integral._
val desc = increment < zero
val beforeStart = start - increment
if (desc) "…" + (-increment) + "…"
```

all came out as `value <op> is not a member of T`. There were four causes, and
all of them have the same shape: **using a conversion that is an instance
member of a generic class, through a value**.

1. **Implicits from jar classes never entered scope at all**. Members are read
   from the pickle one name at a time, but nobody writes the name of an
   implicit (you find it by searching the scope), so
   `Numeric#mkNumericOps` / `Ordering#mkOrderingOps` were never requested once.
   For the same reason `Option.option2Iterable` was nowhere either, and
   `where.reduceLeft(f)` / `c.where.toSeq ++ on` (`Option[Node]`) came out as
   `value reduceLeft is not a member of Option[Node]`. For both
   `import <value>._` and the companion in a type's implicit scope, we now ask
   the pickle **which names are implicit** and complete only those names
   through the usual on-demand path (names the class already has a member for
   are not asked about, so the prelude still wins as before; companions of
   primitives are out of scope — the implicits in `object Int` are the numeric
   widenings themselves, and lining them up as views makes `n + ":"`
   ambiguous).
2. **The candidate was left with the owner's type parameters**. Seen through
   `b: Box[Int]`, `class Box[T] { implicit def mkOps(lhs: T): Ops[T] }` is
   `Int => Ops[Int]`. Only the value can say that
   (`Typer::at_import_prefix_of`).
3. **An overridden conversion was counted twice**. `Integral[T]` narrows the
   result of `Numeric[T]#mkNumericOps` from `NumericOps` to `IntegralOps`.
   After the import both names are in scope, and since the result class and the
   `unary_-` symbol each of them declares differ, the existing "two paths to
   the same conversion" rule did not fire and the search gave up. In nsc there
   is one member (the derived one).
4. **Members of a class nested inside a generic class could not be read**. The
   `T` in `def <(rhs: T)` of `Ordering[T]#OrderingOps` is a parameter of
   *`Ordering`*; `OrderingOps` itself has none. It was treated as an unmappable
   name and the whole member install failed. We now read it with the outer
   parameters and substitute with the same prefix as the conversion.

Separately from these, there was one bug that **typechecks and then fails at
run time**. The conversion is an instance member of a value, yet it was emitted
as a bare name, so codegen pushed `this` and cast it, giving
`class Main$ cannot be cast to class NoTp`.

slick: `errors 203 → 196`, `files_with_errors 60` (unchanged).
`tests/slick_subset.sh` is unchanged at `verified=204 failed=0`. No new kind of
error appeared and no file newly became an error (in two files that already had
errors, the follow-on of an earlier error grew by one line each).

**Known but not fixed** in this slice:

- When an inner class of a subclass extends an inner class inherited from a
  generic parent
  (`class SubBox[T] extends Box[T] { class Sharper(lhs: T) extends Inner(lhs) }`),
  `Inner`'s constructor parameter is left as `Box`'s `T` and you get
  `found: T required: T` (another as-seen-from gap).
- The `no matching overload for (BasicProfile.SchemaDescription)…` for `a ++ b`
  (whose argument is the abstract type member `SchemaDescription`) that the
  brief listed **no longer appears** in the current measurement log.
- The higher-kinded `BuildFrom` matching for `LazyZip2.map` (4 × `toSeq` /
  `mkString is not a member of C` plus 4 ×
  `could not find implicit value of type BuildFrom[…, C]`) is closed in the
  next section (`agent/buildfrom2`).

### Higher-kinded implicit matching for `BuildFrom` (`LazyZip2`, `agent/buildfrom2`)

A remaining item that `agent/mismatch11` and `agent/tail2` diagnosed down to
the cause but left untouched. The fixtures are
`tests/fixtures/bf2_lazyzip.scala` / `bf2_lazyzip_bad.scala`, the test is
`crates/cli/tests/buildfrom2.rs`.

In 2.13 `LazyZip2` is

```scala
class LazyZip2[+El1, +El2, C1] {
  def map[B, C](f: (El1, El2) => B)(implicit bf: BuildFrom[C1, B, C]): C
}
```

and `C` **appears only in the implicit clause**. That means only the witness can
determine the result type, and there is only one general-purpose witness.

```scala
implicit def buildFromIterableOps[CC[X] <: Iterable[X] with IterableOps[X, CC, _], A0, A]
  : BuildFrom[CC[A0], A, CC[A]]
```

There were five gaps between the two, and **the nearer gaps were masking the
deeper ones**.

1. **`BuildFrom`'s companion was not in the symbol table**.
   `load_companion_module`, which reads the companion of a jar class, refused
   everything under `scala/` across the board. The reason was "the prelude is
   what describes the standard library", but what the prelude describes is
   **what a program writes by name**, and nobody writes the name of an implicit
   (you find it by searching the scope). So outside programs that happened to
   write `import scala.collection.BuildFrom`, the `BuildFrom` witnesses were in
   no scope at all. Hand-written declarations are not replaced by anything: a
   class that already has a companion passes straight through the early return
   at the top; a companion with the same JVM name that is already installed is
   not installed twice; and for `scala.*` **only implicits are installed**,
   everything else staying on-demand from the pickle as before (members
   installed by the classfile are dropped — Java's generic signatures cannot
   spell `CC[A]`, so they would sit next to the pickle-derived declaration as
   **a separate, erased overload**).
2. **Half of the low-priority side was still missing**.
   With `object BuildFrom extends BuildFromLowPriority1 extends BuildFromLowPriority2`,
   `buildFromIterableOps` is declared by the **bottom-most** trait. Reading the
   companion alone does not see it, so we walk the parents too and supply their
   implicits.
3. **The supplied implicit was deleted on the spot**. `supply_implicit_members`
   drops the classfile-derived member it replaced with the pickle-derived
   signature, but completion **remembers the names it has already produced**, so
   when the answer was already a pickle-derived member, that same member became
   both "the one being dropped" and "the one being installed", and the class
   ended up with no member of that name at all.
4. **Two-way unification could not match an unknown *type constructor***.
   `CC[A0]` is an `Applied` whose head is a type parameter, while
   `List[String]` is a `Class`. There was no edge joining the two, so it fell
   back to `a == b`. A **fully applied** `implicitly[BuildFrom[…]]` compiled
   only because that one spot falls back to the one-way `unify_one` (which can
   read constructors), and **that fallback is skipped exactly when the call site
   has undetermined parameters** — which is precisely `LazyZip2.map`. That is
   why `xs.lazyZip(ys).map(f)` gave
   `could not find implicit value of type BuildFrom[…, C]` plus
   `value mkString is not a member of C`.
5. **There was nothing to tell the witnesses apart**. The `BuildFrom` witnesses
   **have the same type apart from their bounds**. Higher-kinded bounds arrive
   folded into the type (`buildFromSortedSetOps` is
   `BuildFrom[CC[A0] with SortedSet[A0], A, CC[A] with SortedSet[A]]`), so
   unifying the intersection types is itself the bounds check. But the prelude
   hierarchy did not say that `immutable.TreeSet` is a `collection.SortedSet`
   (`val x: scala.collection.SortedSet[Int] = TreeSet(1)` was a `type mismatch`
   too), so the sorted version did not match, **the unsorted version answered**
   and built with `iterableFactory`, and `TreeSet(1,2).lazyZip(ys).map(f)`
   became `class Set$Set3 cannot be cast to class TreeSet`.
   First-order F-bounds remain in `bound_hi`, so for those we add the
   equivalent of nsc's `checkBounds`. Left unchecked,
   `buildFromBitSet[C <: BitSet with BitSetOps[C]]: BuildFrom[C, Int, C]`
   would answer for `List` (it sits directly on the companion, so it wins on
   origin), and `List(1, 2).lazyZip(…).map(_ + _)` **typechecked and then**
   failed with `class ::$ cannot be cast to class scala.collection.BitSet`.

Three bugs were found by **running the fixtures rather than reading them**.

- **A witness with its own implicit clause was emitted as a bare name.**
  In that one branch `implicit_tree` built an `Ident` instead of going through
  `ref_implicit`, so a declaration from a trait the companion mixed in
  (`buildFromSortedSetOps` is exactly that, and it takes an `Ordering` on top of
  it) had `this` pushed and cast, giving
  `class Main$ cannot be cast to class BuildFromLowPriority1`.
- **A conversion must not determine an unknown type constructor of the call
  site.** The point of (4) is to solve the candidate's **own** type parameter
  `CC`; the call site's `M[_]` is for ordinary inference from the arguments to
  determine. When we opened both without distinguishing them,
  `firstLength[A, M[+X] <: Iterable[X]](in: M[A])` accepted
  `IterableOnce.iterableOnceExtensionMethods` as "a conversion that reaches
  `M[A]`" for a `List[Int]` that already conformed with `M := List`
  (`tests/fixtures/mism12_lib.scala` caught it with a `ClassCastException`). We
  now split unification unknowns into two kinds and **put only the candidate's
  own type parameters in constructor position**.

- **Installing a standard-library companion from the classfile duplicates the
  pickle's declarations.** Until now `object Option` arrived as an empty stub
  from the pickle, and `apply` came from the pickle too. With the classfile's
  erased `apply` lined up next to it, `Option(2)` became
  `ambiguous overload for apply` (`tests/fixtures/jarpk.scala`). For `scala.*`
  companions we discard the classfile's members and leave it to the pickle as
  before.

slick: `errors 177 → 166`, `files_with_errors 57 → 56`
(`QueryInterpreter.scala` now compiles in its entirety).
`tests/slick_subset.sh` is unchanged at `verified=204 failed=0` /
`subset_files=38 classes=204`. **Not one new kind of error appeared, and no
file newly became an error** (what disappeared: 4 × `no implicit` for
`BuildFrom[…]`, 4 × `is not a member` against `C`, plus one `Function0[…] IO[…]`
and one `NotGiven[…]` that were collateral damage).

**Known but not fixed** in this slice:

- `scala.collection.immutable.ArraySeq(1, 2, 3)` gives
  `no implicit: could not find implicit value of type AnyRef[AnyRef]` plus
  `value lazyZip is not a member of Builder[A, ArraySeq[A]]`
  (a separate problem on the `ClassTag` side of `ArraySeq.apply`; it never
  reaches `lazyZip` / `BuildFrom`). It is excluded from the fixtures.
- We do not check the `IterableOps[X, CC, _]` part of a higher-kinded
  parameter's F-bound. The prelude's collections do not carry `IterableOps`'s
  arguments, so checking it would reject candidates nsc accepts. The only case
  where nsc rejects `buildFromIterableOps` on that part is sorted collections,
  and for those the intersection types and hierarchy from (5) reach the same
  conclusion.
- `collection.SortedSet` / `collection.SortedMap` were added this time as links
  in `prelude_hier.rs` (relay nodes with no members of their own). Calling
  `firstKey` and friends directly on values of those types is left to on-demand
  supply from the pickle.

### Block values were double-boxed (erasure)

The `agent/anonbridge` slice. A silent miscompilation that **typechecks and
then throws `VerifyError` at run time**.

```scala
val i = new It[Int] { def next(): Int = { val z = 1; z } }   // VerifyError
val j = new It[Int] { def next(): Int = z }                  // this one worked
```

```text
java.lang.VerifyError: Bad type on operand stack
  Location: Main$$anon$1.next()Ljava/lang/Object; @6: invokestatic
  Reason:   Type 'java/lang/Integer' is not assignable to integer
```

Erasure **passes the expected type of a `Block` / `If` / `Match` / `Try`
straight down to the subexpression that produces the value** (`z` for
`{ …; z }`, both branches for an `if`, each case body for a `match`, the body
and each handler for a `try`). Those are therefore **already boxed**, yet the
tail of `erase_tree` went on to apply `adapt_box_unbox` to the node itself as
well. The node's `ty` is still the pre-boxing `Int`, so the condition held and
`boxToInteger(boxToInteger(z))` came out. An expression body has no node to
descend into, so it was boxed only once — that is what "only breaks for blocks"
really was.

The fix is one place in `crates/typer/src/erasure.rs`. For those four kinds of
node we **stop applying the conversion a second time and merely record the type
the branches ended up with** (the result type of the conversion returned by
`box_adaptation` goes into the node's `ty`). The decision itself shares the same
function as `adapt_box_unbox`, so the behaviour — including value classes'
`new Meters(n)` / `((Meters) x).n()` — is decided in one place.

Real scalac turns the same anonymous class into **two methods** (`next()I`
holding the body, and a bridge `next()Ljava/lang/Object;` that calls it and
boxes). We fold the two together and emit only the one with the erased
signature. The entry point as seen by the caller,
`next()Ljava/lang/Object;`, exists in both, and **boxing exactly once there** is
the correct shape. `scalac_and_ours_agree_on_the_erased_entry_point` in
`crates/cli/tests/anonbridge.rs` pins this down by putting the two side by side
with `javap -p -c -s`.

This was not confined to anonymous classes. The same double boxing showed up in
`val x: Any = { val z = 1; z }`, `id({ val z = 1; z })`, forms whose body is an
`if` / `match` / `try`, named classes (`class C extends It[Int]`),
implementations of an `abstract class`, SAM-converted lambdas, value classes,
and in the reverse direction as double **unboxing**
(`val n: Int = { val z: Any = 1; z.asInstanceOf[Int] }`).

The slick numbers do not move (still
`files=184 errors=378 files_with_errors=67`). It is a bug that passes
typechecking, so it is the kind of fix that does not show in the error count.

### Reading jar classes from the pickle

`load_classpath` only walks directories. That means **classes inside a jar were
read from the JVM generic signature rather than from `ScalaSignature`**. That
format **cannot write higher-kinded kinds**. `trait Monad[F[_]]` arrives as
`<F:Ljava/lang/Object;>`, so `F` is just a type; `def pure[A](a: A): F[A]`
arrives as `(TA;)TF;`, so the result is `F` rather than `F[A]`. As a result
every `Monad[F]` became `kinds of the type arguments (F) do not conform` and
every `F.pure(v)` became `found: F required: F[Int]`. `BasicBackend.scala` and
`ConcurrencyControl.scala`, which use cats / cats-effect, are entirely down to
this.

The pickle carries the real signatures. `crates/pickle` can read 2.13.16
pickles completely (all 799 of scala-library), so **the only thing missing was
a path that used it for jar entries**. That path is
`PickleSupply::adopt_binary_class`.

- Only when the classfile has a `ScalaSig` (that is, when it is a Scala class)
  do we **overwrite** the symbol built by `install_java_class_in` with the
  pickle. The class's parents, flags and fields are used exactly as they came
  from the classfile, and from the pickle we take
  - the **kind** of the type parameters (the arity of `F[_]`), and
  - the signature of each member the pickle declares.
  **Members the pickle could not express are left as the classfile reader had
  them** (no `erased_desc` determined, the type not lowering to a `Type`, and so
  on). So precision goes up, but members never disappear.
- `java.*` is out of scope. For `scala.*`, **only the symbols the prelude built**
  are out of scope (ids below `SymbolTable::prelude_end`). The parts of the
  standard library the prelude hand-writes go through the verified prelude +
  `complete` path, while things the prelude does not name
  (`scala.concurrent.Future`, for example) had the classfile as their only
  source of information, so those are read from the pickle here.
  See "Companion and class are separate symbols (`agent/companionkind`)" for
  details.
- **We do not read ahead.** When one classfile is read, we look at that class
  only. On slick's dependency classpath (cats / cats-effect / slf4j and over 40
  other jars) the measured time went 1:58 → 1:51 (user 101.5s → 107.5s).

Three further gaps were closed along the way.

1. **Applying a type parameter** (`conv_ref`). `F[A]` can be written as
   `Type::Applied`, yet it was being dropped as "higher-kinded, cannot be
   represented". We now build an `Applied` only when `F`'s kind arity matches
   the number of arguments (existential wildcards and things whose kind is
   unknown are still dropped — dropping is better than building the wrong type).
2. **Giving placeholder symbols their kinds after the fact**
   (`give_stub_its_kinds`). `find_or_stub_java_class` installs an "empty
   symbol" for names mentioned by parent lists or descriptors. Outside the
   standard library this hits everywhere: `cats.effect.kernel.Sync` was left
   with zero type parameters, so `Sync[F]` became "applied to 1 argument but
   the symbol has 0" and `Ref.of` / `Ref.ofEffect` / `Ref.lens` all failed. We
   now give the type parameters the pickle declares to symbols nobody has
   filled in yet.
3. **The override test for erasure bridges** (`bridge_overrides`). Two
   parameters that erase to the same descriptor are the same parameter as far
   as the JVM is concerned, so they cannot serve to distinguish overloads.
   Implementing `def bind[A, B](fa: F[A], f: A => F[B])` with `F = Option`
   gives `f: A => Option[B]`, but a structural comparison saw "not an
   override", no bridge was emitted, and `bind` through the interface hit an
   `AbstractMethodError` at run time.

**On the pickle writer side** two more things were fixed. Both were reasons why
"a jar we emitted ourselves cannot be read back" (it works for a directory
because the reader reconstructs the package from the file path).

- **The owner of a top-level class was `<empty>`**. The unpickler reads the
  owner from the pickle, so a class inside `package hklib` called itself
  `Monadic` rather than `hklib.Monadic`. Neither real scalac 2.13.16 nor our own
  reader could find it (`not found: type Monadic`). We now write the package's
  module class as a chain of `EXTMODCLASSref`.
- **`FunctionN` had no type arguments** (`TupleN` did). A `Function1` with no
  arguments leaves the reader no choice but to drop it, so every signature
  containing `f: A => F[B]` went unsupplied.

The measurement (same setup as above) is **772 → 766**, with files containing
errors **100 → 100**. The numbers are small because once `Monad[F]` compiles,
things stop at the next step instead (implicit search through cats'
`implicits`, deriving `Ref.Make[F]`). The content of the errors changed from
misreadings like `kinds of the type arguments (F) do not conform` to genuinely
missing features like `could not find implicit value of type Make[F]`.
- **The whole `scala.collection.mutable` collection set** (`agent/mutcoll`
  slice, only when linking against jars).
  The desugaring of `f(args) = v` into `f.update(args, v)` (SLS 6.15) works for
  arrays, user classes, multi-argument `update`, a selected receiver, a generic
  `update`, and an `update` returning something other than `Unit` (**it works
  under the private runtime too**; a receiver with no `update` is rejected with
  `value update is not a member of …`). Fixed **a bug where a companion's
  varargs `apply` returned the immutable collection of the same name**
  (`mutable.Set(1,2,3)` was inferred as `scala.collection.immutable.Set`, and
  `+=` / `-=` / `++=` / `--=` / `add` came out as "not a member";
  `check.rs::factory_result_class` — the factory shortcut only substitutes type
  arguments, and the class is taken from the declared result type).
  Declared new companions for `mutable.Queue` / `Stack` / `TreeMap` / `TreeSet` /
  `PriorityQueue` / `ArraySeq` (a varargs `apply` including the zero-argument
  case, plus `empty`; `TreeMap` / `TreeSet` / `PriorityQueue` with `Ordering`
  implicit evidence, `ArraySeq` with `ClassTag`) in
  `crates/typer/src/prelude_mutcoll.rs`. These inherit `apply` from
  `IterableFactory` / `SortedIterableFactory` / `EvidenceIterableFactory`, and
  in the classfile signature the varargs parameter has already become `Seq[A]`
  and the result an abstract `CC`, so even `Queue[Int]()` was
  `no matching overload for (Seq[Int])CC with arguments ()`. Along with them:
  `ArrayDeque.append` (a default method on `Buffer`, so it returns `Buffer`),
  `PriorityQueue.enqueue(elems: A*)`, `ArraySeq`'s `apply` / `update` / `length` /
  `size` / `toList`, `mutable.StringBuilder`'s companion `newBuilder` (which
  previously typechecked and then threw `RuntimeException: select StringBuilder`
  at run time), and `Growable` / `Shrinkable`'s `++=` / `--=` / `-=` extended to
  the new types as well (`prelude_mutops.rs`). In 2.13
  `new Queue[Int]()` / `new Stack[Int]()` / `new ArrayDeque[Int]()` are
  `class Queue[A](initialSize: Int = ArrayDeque.DefaultInitialSize)`, so no
  `<init>()V` exists and they used to typecheck and then hit a
  `NoSuchMethodError` at run time (we now call the synthetic default getter
  `$lessinit$greater$default$1`; `gen.rs::has_default_sized_ctor`).
  `new TreeMap[K, V]()` / `new TreeSet[A]()` / `new PriorityQueue[A]()` are
  declared as constructors with an `Ordering` implicit clause.
  **Diagnostics**: when `op=` is not a member of the receiver we now report, as
  nsc does, **a single error** (whose second line is
  `Expression does not convert to assignment because receiver is not assignable.`).
  Previously two separate errors came out, which read as though the preceding
  `m("a") = 1` had failed

### `super.m` is seen from `this.type`, not from the parent (`agent/lastone`)

This is the **last remaining type error** in slick
(`jdbc/SQLiteProfile.scala:183`) together with the **two codegen bugs** that
only became reachable once it typechecked. With this, **all 184 slick files
typecheck in a single compilation**, **4552 classfiles** are emitted, and
**every one of them loads under `java -Xverify:all`** (537 at the start of the
session → 1 just before → **0**).

```
# previous:  subset_files=47  classes=300  verified=300 failed=0
tests/slick_measure.sh   → files=184 errors=0 files_with_errors=0 classes=4552
tests/slick_subset.sh    → verified=4552 failed=0
                           subset_files=184 classes=4552 (of 184 sources)
```

The diagnostic came out like this:

```
error: no matching overload for (Iterable[U], JdbcActionComponent.RowsPerStatement)…
       with arguments (Iterable[U], RowsPerStatement)
```

**It was neither an "overload" problem nor a "named argument" problem.** There
was only one candidate, and it was simply rejecting the arguments. The root was
that the member type of `super.m` was being read by **naming the parent class
on its own**. It should be seen from `this.type`:

```scala
// slick/jdbc/JdbcActionComponent.scala
trait JdbcActionComponent extends BasicActionComponent { self: JdbcProfile =>
  type RowsPerStatement >: slick.jdbc.RowsPerStatement.One.type <: slick.jdbc.RowsPerStatement
  trait InsertActionComposer[U] {
    def insertAll(values: Iterable[U], rowsPerStatement: RowsPerStatement = defaultRowsPerStatement): …
  }
  object MultipleRowsPerStatementSupport extends … {
    override type RowsPerStatement = slick.jdbc.RowsPerStatement   // ← concretized
  }
}
// slick/jdbc/SQLiteProfile.scala:183
trait SQLiteProfile extends JdbcProfile with JdbcActionComponent.MultipleRowsPerStatementSupport {
  private trait SQLiteInsertAll[U] extends InsertActionComposerImpl[U] {
    override def insertAll(values: Iterable[U], rowsPerStatement: RowsPerStatement = RowsPerStatement.All) =
      super.insertAll(values = values, rowsPerStatement = if (…) RowsPerStatement.One else rowsPerStatement)
  }
}
```

Looked at on its own, `InsertActionComposerImpl` leaves `rowsPerStatement`
**as an abstract type member** (`>: One.type <: RowsPerStatement`), and the only
thing that conforms there is the lower bound `One.type`. `SQLiteProfile` mixes
in `MultipleRowsPerStatementSupport`, so seen from `this.type` it is
`slick.jdbc.RowsPerStatement`. We now remember `this_id` when building the
receiver for `super` in `Check::type_select`, and run the member type through
`expand_type_members(this_id, …)` (`this.m` and `x.m` have been doing the same
thing via `expand_in_type` for a while).

Two more things were then broken on the classfile side. **Both were bugs that
had been in main all along**; they simply could not be hit until this shape
typechecked:

1. **Abstract type members erased to `Object`.** Under SLS 3.7 they **erase to
   the upper bound**, just like type parameters. Real scalac 2.13.16 writes
   `insertAll(Iterable, Rps)` too. Because we used `Object`, the inherited
   `insertAll(Iterable, Object)` and the profile's `insertAll(Iterable, Rps)`
   became **different JVM methods**, and the trait's `$super$` accessor hit a
   `NoSuchMethodError` (`crates/typer/src/erasure.rs::erase_ty`). We take the
   bound **only when it names a single class**. A **compound-type bound** such
   as `type TermName >: Null <: TermNameApi with Name` (scala-reflect) needs
   nsc's `intersectionDominator`, so those are left as `Object` as before.
   Taking the first parent would give `TermNameApi`, which is not a `NameApi`,
   so the checkcast when passing a `TermName` to
   `Select.apply(TreeApi, NameApi)` (which requires a `Name`) — a cast that was
   there precisely because the type came out as `Object` — disappeared, and the
   macro bridge became `VerifyError: Bad type on operand stack`.
2. **The descriptor of the `T$$super$m` accessor disagreed between the call site
   and the forwarding target.** The accessor is a member of the trait, so it
   carries **the overriding side's** erasure, while the parent method it
   forwards to keeps **its own**. With `override type Rows = One.type` (a
   concretization narrower than the bound) the two do not match, `invoke_super`
   called a method that does not exist, and the accessor body did an
   `invokespecial` to a method that does not exist. We now make the call with
   **the current method's** descriptor and the forward with **the target's**
   descriptor, inserting a `checkcast` only when the return type narrows
   (`crates/backend/src/gen.rs::invoke_super` / `emit_super_accessors` /
   `super_target_desc`).

**The brief's hypothesis was wrong.** The previous slice wrote "as-seen-from
for bounded abstract type members", which is the right area, but what actually
mattered was neither `subst_as_seen_from` nor `self_type_of_class`: it was
**that only the receiver of `super` was not consulting `this`'s type-member
table**. The `self:` annotation (`self: JdbcProfile =>`) was irrelevant too.

**A remaining item in the same area that this section does not fix** (real
scalac accepts it):

```scala
trait Profile extends Comp with MultiSupport {   // MultiSupport concretizes type Rows
  def h(c: ComposerImpl[Int], x: Rps): String = c.single(x)   // ← still rejected
}
```

`ComposerImpl` is an inner class of `Comp`, so its type in nsc is
`Profile.this.ComposerImpl[Int]`. Our `Type::Class` **carries no prefix type**,
so it cannot reach the fact that `Profile` concretizes `Rows`. The `super` and
`this` routes are fixed here, but **an inner-class receiver reached through a
value** cannot be fixed without giving the type a prefix (that means adding a
prefix to `Type::Class`, so it is not done in this slice).

### Operator-named `val`s were not encoded as field names

This only surfaced once classfiles were emitted for all 184 files. **Two** of
the 4552 failed to load with
`java.lang.ClassFormatError: Illegal field name "/"` (`slick.ast.Library$` and
`slick.lifted.NumericColumnExtensionMethods$class`).

```scala
// slick/ast/Library.scala:31
val / = new SqlOperator("/")
```

**Method names were encoded (`crates/pickle/src/names.rs`), but field names
were left raw.** JVMS 4.2.2's "unqualified name" does not allow `.` `;` `[`
`/`, so only `/` fails at load time (`+` `-` `*` `%` happen to be legal, so the
names were odd but worked). nsc runs every term name through the same
NameTransformer. We now run **both** the field definition side in
`ClassEmit::write_with_pool` and the reference side of
`getfield` / `putfield` / `getstatic` / `putstatic` through
`encode_method_name` (`crates/backend/src/code.rs`). Running an already-encoded
name through it changes nothing, so existing synthetic fields
(`$outer` / `bitmap$0` / `MODULE$`) are unaffected.

### `slick_subset.sh` was discarding files because of warnings

This only surfaced once the type errors reached zero. `slick_subset.sh` picked
up the files with errors from `^\s+--> …\.scala` lines, but that `-->` line
**is attached to warnings too**. Seeded with a measurement log of 0 errors and
2 warnings, `JdbcActionComponent.scala` was excluded as a "bad file", the files
depending on it dropped out on the next round, and the converged 184 files
shrank to 132. We now pipe through `grep -A 2 '^error'` first and look **only
at the `-->` lines immediately after an error**.

### `-Xsource-features:case-apply-copy-access` and `-Xasync` (`agent/xflags`)

Two scalac flags. Everything below was read off scalac 2.13.16 first and is
pinned by `crates/cli/tests/xflags.rs`, which runs the same fixtures through
both compilers.

#### The two migration axes

`-Xsource:3` and `-Xsource-features` are not the same axis:

| flag | effect |
|---|---|
| `-Xsource:3` | *warns* where Scala 3 would differ (as errors, in 2.13.16) |
| `-Xsource-features:<f>` | actually *adopts* the Scala 3 behaviour `f` |
| `-Xsource:3-cross` | `-Xsource:3 -Xsource-features:_` |

They are not independent. nsc gates every feature on `isScala3`:

```scala
// scala/tools/nsc/Global.scala, 2.13.16
def caseApplyCopyAccess = isScala3 && contains(o.caseApplyCopyAccess)
```

so `-Xsource-features` on its own is dropped, with `ScalaSettings.conflictWarning`:

```
warning: Conflicting compiler settings were detected. Some settings will be ignored.
-Xsource-features requires -Xsource:3
```

We parse the whole feature domain (the eleven names, the `v2.13.13` /
`v2.13.14` / `v2.13.15` groups, `_`, removals such as
`v2.13.14,-case-companion-function`, and `help`) with nsc's own error text for
an unknown name, and implement **`case-apply-copy-access`**. A feature named
one by one that we do not implement warns rather than passing silently; naming
a *group* does not, because `-Xsource:3-cross` expands to one.

#### `case-apply-copy-access`

Without the feature, the synthesized members walk straight around a private
constructor — `case class C private (x: Int)` still has a public `C.apply(1)`
and a public `c.copy(x = 2)`. With it, the primary constructor's modifier is
copied onto both. **The two rules are different**, which is the part that is
easy to get wrong:

| constructor | `apply` | `copy` |
|---|---|---|
| `private` | `private` | `private` |
| `private[p]` | `private[p]` | `private[p]` |
| `protected` | **public** | `protected` |
| (none) | public | public |

nsc's `Unapplies.applyAccess` reacts only to `private` / `private[p]`
(`mods.hasFlag(PRIVATE) || (!mods.hasFlag(PROTECTED) && mods.hasAccessBoundary)`)
and then copies only the `PRIVATE` bit, while `caseClassCopyMeth` copies
`flags & AccessFlags` (`PRIVATE | PROTECTED | LOCAL`) outright. Confirmed with
scalac: `case class D protected (x: Int)` keeps a public `apply` and gets a
`protected` `copy`.

The feature is marked `[bin]` in nsc's help because it changes the class file,
and it does so in three ways, all reproduced here:

```text
// case class C private (x: Int)                without      with the feature
public final class C$ extends AbstractFunction1  →  public final class C$ implements Serializable
public C apply(int)                              →  private C apply(int)
public C copy(int)                               →  private C copy(int)
public static C apply(int)   // mirror forwarder →  (gone)
```

The lost `FunctionN` parent is nsc's `caseModuleDef`
(`&& !ApplyAccess.isInherit(applyAccess(constrMods(cdef)))`): a companion whose
`apply` is not public cannot be a `FunctionN`, whose `apply` is. It is also the
one place the feature is visible for `private[p]`, which is otherwise a public
method in the class file.

**Where the access check happens.** Setting the flag on the symbol is most of
the work: `type_select`'s existing check then rejects `C(1)`, `C.apply(1)` and
`v.copy(x = 2)` with this compiler's usual wording. `copy` needed one addition:
`try_rewrite_case_copy` turns `p.copy(x = 1)` straight into a constructor call,
so the member is never selected and the check never ran.

**Widening.** A `private` member read from another class file is an
`IllegalAccessError`, and these are read across one constantly: `C(x)` written
inside `C` is a call from `C` into `C$`, and a class nested in `C` calling
`copy` is a call from `C$Inner` into `C`. nsc's answer is `makeNotPrivate`,
which widens *and renames* — scalac emits `public C C$$copy(int)` for exactly
that program. We widen without renaming, which is what `widen_private_ctors`
in `crates/typer/src/expand_private.rs` already did for private constructors;
the rename exists to stop a subclass accidentally overriding the published
member, and there is nothing to collide with here. `tests/fixtures/xflags_case_access.scala`
runs every one of those shapes and prints the same thing under both compilers.

#### `-Xasync`

`-Xasync` is accepted, and reaches a macro implementation through
`c.compilerSettings` — which is where the flag is actually *observed*. The
message a user gets for a missing `-Xasync` does not come from the compiler at
all; it comes from the library:

```scala
// scala/async/Async.scala, scala-async 1.0.1
def asyncImpl[T: c.WeakTypeTag](c: whitebox.Context)(body: c.Tree)(execContext: c.Tree): c.Tree = {
  if (!c.compilerSettings.contains("-Xasync"))
    c.abort(c.macroApplication.pos,
      "The async requires the compiler option -Xasync (supported only by Scala 2.12.12+ / 2.13.3+)")
  ...
}
```

So `c.compilerSettings` and `c.macroApplication` are now implemented in the
macro engine (`crates/typer/java/ScalaRsMacroEngine.java`), and the driver
rebuilds the command line the way nsc's `Settings.recreateArgs` does
(`-classpath`, `-d`, `-Xasync`, `-Xsource:3.0.0`, …). A macro that gates on a
flag now behaves identically under both compilers:
`tests/fixtures/xflags_async_{impl,use}.scala` is scala-async's gate,
compiled and run by both.

The state-machine transform itself is **not** implemented, and neither is
reading a macro *definition* out of a jar's pickle, which is what
`scala.async.Async.async` is. See `docs/not-implemented.md`.

### `-Ykind-projector`: kind-projector's type-lambda syntax (`agent/kindproj`)

[kind-projector](https://github.com/typelevel/kind-projector) is a compiler
*plugin*, not Scala. nsc without it rejects `Either[E, *]` and
`λ[α => F[α]]`, and so does this compiler unless `-Ykind-projector` is
given -- the default has to stay a rejection for "compiles what nsc compiles"
to mean anything anywhere else. The flag name is Scala 3's for its own
compatible version of the syntax; nsc 2.13 has no flag of that name.

With the flag, the parser desugars the plugin's two forms onto the structural
type lambda `({ type L[a] = ... })#L`:

```scala
Either[Int, *]              // [a] => Either[Int, a]
(A0, *)                     // [a] => (A0, a)
E => *                      // [a] => E => a
Function2[-*, Long, +*]     // [-a, +b] => Function2[a, Long, b]
EitherT[*[_], Int, *]       // [F[_], b] => EitherT[F, Int, b]
λ[α => F[G[α]]]              // [a] => F[G[a]]
Lambda[(A, B) => Either[B, A]]  // [a, b] => Either[b, a]
λ[F[_] => Wrap[F]]           // [F[_]] => Wrap[F]
λ[`+α` => Box[α]]            // [+a] => Box[a]
```

Two rules matter and were read off `scalac -Xplugin:kind-projector...jar
-Xprint:kind-projector` rather than guessed: a `*` binds to the **innermost**
enclosing type application (`Either[Int, List[*]]` is
`Either[Int, [a] => List[a]]`), and a function type counts as an application of
`FunctionN`. A shape the plugin does not recognise (`λ[Int]`,
`λ[α => F[α], β]`) is passed through unchanged, so the diagnostic is nsc's
`not found: type λ`.

The term-level `λ[F ~> G](f)`, which builds a `FunctionK` value, is **not**
implemented; `λ` in expression position stays `not found: value λ`.

`tests/fixtures/kp_lambda.scala` runs every accepted form and its expected
output is scalac 2.13.16 + kind-projector 0.13.3's;
`tests/fixtures/kp_lambda_bad.scala` pins the four errors both compilers give
for shapes that are not lambdas and for lambdas that do not match;
`tests/fixtures/kp_plain.scala` pins that the flag changes nothing for a
program that does not use the syntax. See `docs/cats.md` for what it does to
the cats measurement (2929 errors -> 1128).
