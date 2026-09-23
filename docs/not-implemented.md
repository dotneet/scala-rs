# Known gaps

What scala-rs does not do, or does differently from scalac 2.13.16. Nothing
here is stubbed to "pretend it works": each gap is either reported with a
diagnostic or, where marked, is a known divergence. What is supported is in
[language-support.md](language-support.md).

## Out of scope

- Compiler plugins (kind-projector's syntax is available behind
  `-Ykind-projector` instead).
- Scala 3 syntax and TASTy, beyond the `-Xsource:3` cross-building spellings.
- Forms of `forSome { val x: T }` other than `p.Inner forSome { val p: Outer }`.

## Language and type system

- **Deeper higher-kinded bound conformance.** Written class and alias
  applications enforce their bounds, but this is not full existential or
  path-dependent type support.
- **Class types carry no prefix.** An inner class of a generic class loses its
  outer's type arguments (`new B[X].m(1).n(x)` for
  `class B[T] { def m[A](a: A) = new B1(a); class B1[A0](a0: A0) { def n(z: T) = … } }`
  reports `found: X required: T`), and `A#B` is plain `B` for subtyping.
- **Solving a class type parameter from an omitted constructor default.**
  `case class C[+F <: Option[Int]](n: String, f: F = None)` called as `C("q")`
  reports `type mismatch; found: None$ required: F`; nsc takes `F` from the
  default getter's inferred result type (`None.type`).
- **A default whose expression does not conform to the parameter's type
  parameter.** `def f[T](x: T = ())` is rejected (`found: () required: T`);
  nsc infers the getter's result type and accepts it. Reading such a method
  from a class file works ([default-arguments.md](default-arguments.md)).
- **An enclosing template's self type, for a bare name in a nested one.**
  `trait Q { self: PriorityQueue[Int] => trait Inner { def d = dequeue() } }`
  reports `not found: value dequeue`; the same call directly in `Q` resolves.
- **Name-binding ambiguity.** The definition-versus-deeper-import ambiguity
  (`ambiguousDefnAndImport`) is reported. Still missing: a definition made by
  the unit's own package clause against a deeper import; two imports at
  different nesting levels (`ambiguousImports`); and the type namespace (only
  term references are checked). nsc spells the owner of a definition in a
  `locally { … }` block `value <local Y>`, we say `object Y`.
- **A local `object` that captures.** An `object` in a method body is emitted
  as a static `MODULE$` singleton, whereas nsc creates one per call in a
  `LazyRef` and passes the enclosing instance and captured locals.
  `crates/typer/src/localobj.rs` therefore refuses a local `object` whose body
  reads anything outside itself ("not implemented: a local `object` that reads
  a local of the enclosing method").
- **Remaining tail-call shapes.** A recursive call inside an explicit `return`
  or inside `try` / `catch` / `finally` is rejected under `@tailrec`; mutual
  recursion is not transformed ([tailrec.md](tailrec.md)).
- **Sealed and final library classes the prelude declares.** The prelude does
  not mark `Option`, `List`, `Either`, `Try` and others `sealed`, so
  `new Option[Int] { … }` is accepted (scalac: "illegal inheritance from sealed
  class Option") and fails at class-load time with `IncompatibleClassChangeError`
  when it overrides one of the jar's final methods. See
  [prelude-fidelity.md](prelude-fidelity.md).
- **`Iterator.GroupedIterator` is reachable through `object Iterator`.** The
  class is declared in `trait Iterator`, but a nested class file's JVM name
  does not say whether the outer is the class or the object, so the symbol is
  entered in both scopes; scalac reports `type GroupedIterator is not a member
  of object Iterator`.
- **A nested class named in a pickled signature outside `scala.collection` is
  entered as a package-level class with `$` in its name** (for example
  `scala/reflect/api/Exprs$Expr` beside the prelude's hand-built `Exprs.Expr`).
- **Library member sets are completed on demand.** `PickleSupply` installs a
  library class's members one name at a time, so a question about a library
  class's full member set (for example, whether it is a SAM type) is answered
  against a partial set. `Checker::sam_sig_here` reads the pickle directly for
  the SAM question only.
- **Cyclic inheritance.** A cycle is rejected with `illegal cyclic reference
  involving trait X` at scalac's position, but only the first cycle each
  template reaches is reported, and nsc's follow-on `illegal cyclic
  inheritance` errors (and a type mismatch they would otherwise mask) are not.

## Code generation and ABI

- **Class- and trait-owned `@specialized`.** Only method-owned `Int`/`Long`
  variants are emitted; no `Foo$mcI$sp` class exists, so our specialized
  classes are not ABI-compatible with scalac's
  ([specialization.md](specialization.md)).
- **SAM literals.** A `FunctionN` literal is an `invokedynamic`; every other
  SAM type (and `PartialFunction`) becomes an anonymous class, where scalac
  uses `invokedynamic` for SAM types it considers functional interfaces.
  Behaviour is the same; class count and call-site shape differ.
- **Repeated parameters forwarded to a Java varargs method.**
  `def fmt(s: String, args: Any*) = s.format(args: _*)` passes the `Seq`
  without nsc's `Object[]` conversion and throws
  `MissingFormatArgumentException` at run time.
- **A partial function whose parameter is a written tuple type** gets no entry
  `checkcast scala/Tuple2`, where the same type spelled `Tuple2[A, B]` does.
  Not observable (the pattern's own `instanceof` is emitted), but the two
  spellings emit different code.
- **The pickle is a subset of nsc's.** It covers what scalac needs to
  typecheck against our class files: symbols, method and poly types with their
  source parameter clauses, class parents, type bounds, `this`/singleton,
  constant, existential and refined types, annotations with constant or simple
  tree arguments, access flags including `privateWithin`. A default getter of
  a curried method is pickled with its clauses flattened, so a scalac client of
  `def join(a: String)(b: String = "-")(c: String = a + b)` compiled by us
  reports `not enough arguments for method join$default$3` (a scala-rs client
  of the same class files compiles and runs).
- **Case class ABI details.** The companion's `writeReplace` and the static
  forwarders nsc puts on the class for the companion's `apply` / `unapply` /
  `tupled` / `curried` are not emitted; case accessor fields are
  `public final` where nsc's are `private final`
  ([comparison-with-scalac.md](comparison-with-scalac.md)).
- **Evidence materialization.** Full `Manifest`s for instance-dependent inner
  classes and instance-member singleton types need type prefixes the
  representation drops, and are reported as missing evidence.
  `TypeTag`/`ClassTag`-to-`Manifest` interoperability is unsupported.
  `TypeTag` materialization covers monomorphic top-level class types only.

## Library

- **The private runtime** (`--no-scala-library`) is deliberately small. It has
  no `scala.Array` companion factory (`Array(1, 2)` reports `no matching
  overload`), no `Ordering`/`Equiv` companions, no sequence patterns on
  `Seq`/`Array`, and no `scala.specialized` import target. Use the library ABI
  for those.
- **`ArrayOps.indexOf` / `lastIndexOf` without the `from` argument.**
  `Array(1, 2, 1).indexOf(1)` reports `no matching overload for (Int, Int)Int
  with arguments (1)`: the prelude's declaration drops the library's default
  ([prelude-fidelity.md](prelude-fidelity.md)).
- **Named arguments on prelude members whose library declaration is
  elsewhere.** `Array(1, 2, 3).mkString(sep = "|")` reports "named arguments
  (method parameters not resolved)": the prelude declares `mkString` on
  `ArrayOps`, which the library does not.
- **Repeated-variable constraint solving.** `first[A]((String, Int))`-style
  calls that need a common result type from one type variable in several
  positions are not solved.

## Macros and quasiquotes

Def macro expansion, quasiquotes and `reify` are implemented for a subset;
everything outside it is reported by name, never expanded into a different
tree. The main remaining pieces are: whitebox APIs beyond the implemented
`Context` operations, anonymous classes returned by an expansion,
`c.inferImplicitView`, block/function/`new` arguments passed through to the
implementation, `c.prefix` for a call without a receiver, classes from the
same run as type arguments, tags for type parameters that have none,
recovery from `c.parse`'s `ParseException`; and, for quasiquotes, forms the
parser normalises away (`if` without `else`, by-name types, procedure
syntax, …), `type` definitions, holes whose type has no standard `Liftable`,
rank-2 holes, and several shapes in pattern position. See
[macros.md](macros.md) for the full list and what each would need.

## Compiler flags and diagnostics

- **`-Xasync`**: `postAnfTransform`, `stateDiagram` and a transformed method
  taking an extra state-machine self parameter are diagnosed. Generated
  classes, allocations and callback counts do not match nsc's
  single-state-machine representation ([async.md](async.md)).
- **`-Xsource-features`**: `case-apply-copy-access` and `unicode-escapes-raw`
  are implemented; `infer-override` is partial (ordinary inferred methods,
  vals and vars adopt the inherited type and final constant vals keep their
  narrower type; macro-related exceptions are unverified). The others
  (`case-companion-function`, `case-copy-by-name`, `any2stringadd`,
  `string-context-scope`, `leading-infix`, `package-prefix-implicits`,
  `implicit-resolution`, `double-definitions`) are parsed and validated, warn
  when named individually, and change nothing. Naming a group (`_`,
  `v2.13.14`) does not warn, because `-Xsource:3-cross` expands to `_`.
- **`-Xsource:3` migration errors** (the `scala3-migration` category) are not
  reported.
- **`copy$default$N` access** under `case-apply-copy-access` stays public;
  nothing in Scala source can name these getters, and omitted `copy`
  arguments are filled at the call site.
- **`enableRequired` language features.** Using `postfixOps` or `dynamics`
  without the import is an error in nsc (`postfix operator toString needs to
  be enabled`); we report a feature warning, so such a file compiles here.
- **Default warnings not issued**: explicitouter's `The outer reference in
  this type test cannot be checked at run time.`, patmat's fruitless type
  tests, and warnings about trees a macro expands to. `Reference to
  uninitialized` does not look inside patterns.
- **Value class restrictions.** nsc's `neg/valueclasses` rules and most of
  `checkEphemeral` are checked (`crates/typer/src/valueclass.rs`); its
  secondary-constructor, redefined `equals`/`hashCode`, "additional parameter"
  and qualified-`super` arms are not.
