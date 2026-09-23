# Language support

scala-rs compiles a subset of **Scala 2.13** to JVM class files; the reference is
scalac 2.13.16. There is no Scala 3 syntax (except the `-Xsource:3` spellings
listed below) and no TASTy. The entry point is `def main(args: Array[String]): Unit`
or `extends App`.

This page describes what is supported and how it maps onto nsc's behaviour.
Known gaps are in [not-implemented.md](not-implemented.md); the compiler's
structure is in [architecture.md](architecture.md). Where a feature has its own
page, it is linked: [macros](macros.md), [async](async.md),
[tail calls](tailrec.md), [specialization](specialization.md),
[default arguments](default-arguments.md).

Unsupported syntax is never dropped silently: the parser reports a diagnostic
and produces an `Unimplemented` node, and the typer reports shapes it cannot
handle by name rather than emitting something different.

## Library modes

- **Library ABI** (the default when a scala-library 2.13.16 jar is found, or
  `--scala-library [<jar>]`; without a path it searches `SCALA_LIBRARY_JAR`,
  `/tmp/scala-rs-lib` and the working directory). Programs link against the
  real jar and no class of the same name is emitted. Library members are
  typed from a hand-written prelude (`crates/typer/src/prelude*.rs`) plus the
  jar's `ScalaSignature` pickles, which supply every member the prelude does
  not declare (see [architecture.md](architecture.md)).
- **Private runtime** (`--no-scala-library`). A small runtime is emitted with
  the program (`crates/backend/src/runtime.rs`): `Option`, `List`, `FunctionN`,
  `Tuple2`, `MatchError`, the `Lazy*` cells, `Nothing$`, `Null$` and similar.
  Many library features (`Array.apply`, sequence patterns on `Seq`/`Array`,
  `FunctionN.tupled`, `Ordering`/`Equiv` summoning, `scala.Int` companion
  constants, most collections) are library-ABI only and are diagnosed here.

## Definitions and templates

- Packages, including nested `package p { package q { … } }` and package
  objects. A qualified `package p.q` clause opens only `p.q` (SLS 9.2).
- Classes, traits, objects, case classes and case objects; auxiliary
  constructors; curried constructors (`new C(1)(2)`); early definitions
  (`class C extends { val x = 1 } with T`); template statements, run in
  declaration order interleaved with field initialisation (SLS 5.1).
- Nested and inner classes (`$outer`), objects as members of a class or trait
  (one per enclosing instance, via a `<name>$module` field and accessor), and
  local and anonymous classes that capture locals of the enclosing method
  (a field per free variable; captured `var`s go through `scala.runtime.*Ref`).
- `val` / `var` / `def` / `lazy val` (members use a `bitmap$0` and a
  synchronized accessor; method-local ones use a `scala.runtime.LazyRef` cell
  family, as nsc's `lazyvals` phase does). Members without a type annotation
  are completed lazily when first needed, so forward references work.
- **Constant-typed `final val`s** (SLS 4.1). An unannotated `final val` whose
  right-hand side is a constant expression keeps its constant type; the
  expression is folded the way nsc's `ConstantFolder` does
  (`crates/typer/src/const_fold.rs`), and a reference through a pure stable
  path is replaced by the literal, so reading `K.N` does not initialise `K`.
  The pickle records the constant type, and constant-typed values read from
  scalac's pickles are inlined the same way.
- Case classes get nsc's synthesized members (`copy` with `copy$default$N`,
  `productPrefix`, `productArity`, `productElement`, `productElementName`,
  `productIterator`, `canEqual`, `hashCode`, `toString`, `equals`, and the
  companion's `apply` / `unapply`); the companion extends
  `AbstractFunctionN`, so `P.tupled` / `P.curried` work, and case classes are
  `Product with Serializable`. A generic case class's `copy` has its own type
  parameters, as nsc's `copy[A](value: A = …): C[A]` does. See
  [comparison-with-scalac.md](comparison-with-scalac.md) for the remaining
  ABI differences.
- `scala.Enumeration`, `extends App` / `DelayedInit`, `scala.Dynamic`
  (`selectDynamic` / `applyDynamic` / `applyDynamicNamed` / `updateDynamic`,
  with the `dynamics` language import).
- Access modifiers: `private`, `protected`, `private[this]`, `private[p]` and
  `protected[p]` (the qualifier is resolved outward from the definition site).
  A `private[p]` member is not offered by a wildcard import written outside
  `p`, as in nsc. `private` / `protected` constructors are checked at `new`.
- Override checking (SLS 5.1.4): result covariance, parameter invariance, the
  `override` modifier, `final`, abstract/concrete redeclaration, type members
  with type parameters.
- Value classes (`extends AnyVal`) and universal traits (`extends Any`); see
  [below](#value-classes).

## Expressions

- Literals, tuples, blocks, `if` / `while` / `do-while`, `try` / `catch` /
  `finally` (an exception table; `finally` is duplicated, no `jsr`), `throw`,
  `return` including non-local `return` from a lambda
  (`NonLocalReturnControl`), `synchronized`, `eq` / `ne`,
  `asInstanceOf` / `isInstanceOf`.
- Lambdas (typed, inferred from the expected type), placeholder syntax
  (`_ + 1`, `(_: Int) + 1`), pattern-matching anonymous functions
  (`xs.map { case (a, b) => … }`), `PartialFunction` literals, eta-expansion
  (`f _` and unapplied methods where a function is expected), SAM conversion
  (SIP-21) to Java and Scala single-abstract-method types.
- Infix, prefix and postfix operators (postfix needs `postfixOps`, as in nsc);
  right-associative operators ending in `:`; `xs(i) = v` as `update`;
  assignment operators (`+=`).
- Named and default arguments, reordered at the call site with nsc's
  `NamesDefaults` rules, for methods, `apply`, `copy`, constructors and
  overloaded calls. Defaults are `{method}$default$N` getters, so separately
  compiled callers can use them ([default-arguments.md](default-arguments.md)).
- By-name parameters (`=> T`) and varargs (`T*`, `xs: _*`).
- for-comprehensions, desugared to `map` / `flatMap` / `foreach` /
  `withFilter`, with value definitions and guards.
- String interpolation: `s`, `f` (lowered to `String.format`), `raw`, and
  custom interpolators through `StringContext` (library ABI).
- XML literals (elements, attributes, namespaces, comments, CDATA, processing
  instructions, entity references) against scala-xml.
- Structural types (`{ def foo: Int }`), called through Java reflection like
  nsc's reflective calls, including structural assignment.
- Overloading resolved nsc-style by argument types and arity, with the
  more-specific rule; in value position only nullary alternatives are kept
  (SLS 6.26.3).
- Numeric widening (SLS 3.5.3 weak conformance), all 49 primitive
  conversions (`toByte` … `toDouble`), `Byte` / `Short` / `Char` as real JVM
  primitives, `java.lang.Integer` and `scala.Int` as distinct types linked by
  `Predef`'s conversions.

## Patterns

Constructor, literal, typed, wildcard, alternative and binder (`x @ p`)
patterns; stable identifiers (including Java enum constants); `case null`;
extractors (`unapply` returning `Option`, `Boolean` or a tuple, and
name-based extractors); `unapplySeq` on `List` / `Seq` / `Vector` /
`IndexedSeq` / `Array` and user extractors, with `_*`; named case-class
patterns. An unmatched `match` throws `scala.MatchError` carrying the
scrutinee. Matches over a `sealed` hierarchy are checked for exhaustivity and
unreachable cases, as warnings (errors under `-Xfatal-warnings`); `@switch`
and variable-pattern warnings are issued too.

## Types

- Generic classes and methods, variance (with `@uncheckedVariance`), upper and
  lower bounds, view bounds (`T <% V`) and context bounds (`T: C`), including
  on class type parameters and higher-kinded parameters (`F[_]: Monad`).
- Higher-kinded types, with nsc's kind checking of type arguments
  (`crates/typer/src/kind_bounds.rs`).
- Abstract type members, type aliases (transparent, including aliases in a
  jar's package object, which exist only in its pickle), type projections
  (`A#B`, `Outer#Inner#X`), path-dependent types, singleton types
  (`x.type`, `this.type`), `super.T` in type position, structural type
  lambdas (`({ type L[a] = Either[E, a] })#L`). Class types and type members
  carry no prefix: members of `A#B` are re-read from `A` (as-seen-from), but
  for subtyping and display `A#B` is plain `B`, and an inner class of a
  generic class loses its outer's type arguments (see
  [not-implemented.md](not-implemented.md)).
- Compound types (`A with B`) and refinements; existential types (`List[_]`,
  bounded wildcards, `T forSome { type X }`, `p.Inner forSome { val p: Outer }`).
- SIP-23 literal types (`val x: 1 = 1`).
- Self types, the cake pattern across compilation units (a header pass types
  every unit's parents before signatures and bodies), and a trait extending a
  class (SLS 5.3.3).

### Type inference

Method type parameters are solved from the arguments and the expected type
together (nsc's `instantiateExpecting`); the expected type is also passed to
the arguments as a prototype (`protoTypeArgs`). Undetermined type variables
are carried through overload resolution and solved afterwards, as nsc's
`undetparams` are. Lower-bounded parameters (`[B >: A]`) take the lub of the
argument and the receiver's type. Dependent method types (`p.State` in a
result) are instantiated from the argument.

### Implicits

The search follows nsc: local scope and enclosing templates (including
inherited and imported implicits, viewed through the prefix's type
arguments), then the implicit scope of the target type — the companions of
its parts and their base classes, loaded from the jar's pickle on demand.
Polymorphic implicit defs are unified two-sided with the expected type;
several candidates are ranked by nsc's `isStrictlyMoreSpecific`; divergence
is detected. Implicit conversions (views) and implicit classes are applied
to receivers and arguments, and a function-typed implicit parameter
(`A => B`) can be filled by eta-expanding an implicit conversion. An explicit
implicit argument list (`f(x)(ev)`) is not searched again. Failures are
reported (`could not find implicit value`, `ambiguous implicit`,
`diverging implicit expansion`); nothing is stubbed.

`ClassTag`, `Manifest` / `OptManifest`, and `TypeTag` / `WeakTypeTag` (for
monomorphic types) are materialised when no implicit is in scope, as nsc's
compiler-intrinsic macros do.

## Code generation

The pipeline is described in [architecture.md](architecture.md). Points that
matter for compatibility with scalac-built class files:

- **Erasure** follows nsc: type parameters and unbounded wildcards erase to
  `Object`; an array of an abstract element type erases to `Object`; `Unit`
  is `V` only as a method result and `scala/runtime/BoxedUnit` elsewhere;
  `Nothing` and `Null` erase to `scala/runtime/Nothing$` / `Null$` except as
  array elements. Erasure bridges are emitted for overridden generic members,
  including members of library traits such as `Ordering`, `PartialFunction`,
  `Equiv` and `Numeric` that the source class implements (the library
  members are completed from the pickle so the bridge pass sees them).
- **Lambdas**: a `FunctionN` literal is an `invokedynamic` through
  `LambdaMetafactory.metafactory` with a `public static $anonfun$N` body. A
  `PartialFunction` literal, a user SAM type and arities above 22 become
  synthetic classes. nsc's `altMetafactory` serialization and
  `JFunction1$mcII$sp`-style specialized interfaces are not reproduced
  (`SCALA_RS_LAMBDA_TRACE=1` prints which fallback was taken).
- **Traits** are interfaces with `default` methods and a `static m$` beside
  each; implementing classes get forwarders in linearization order. A trait
  `val` uses nsc's mixin setter `T$_setter_$v_$eq` and a static `$init$`;
  `abstract override` uses `T$$super$m`. A scalac subclass of our trait links
  (`crates/cli/tests/traitclass.rs`).
- **Objects** are `Foo$` with `MODULE$` and static forwarders on `Foo`.
- **Class-file attributes**: `ScalaSignature` (see
  [comparison-with-scalac.md](comparison-with-scalac.md) for the pickle's
  coverage), JVM generic `Signature` (recorded before erasure by
  `crates/backend/src/sig.rs` and attached only where it erases back to the
  descriptor beside it; `SCALA_RS_SIG_DEBUG=1` prints the rejects),
  `InnerClasses` / `EnclosingMethod`,
  `ConstantValue` for `@SerialVersionUID`, `StackMapTable` (class file major
  version 52).
- `@tailrec` and direct self tail calls become loops ([tailrec.md](tailrec.md)).
- `@specialized`: method-owned `Int` / `Long` variants only
  ([specialization.md](specialization.md)).

### Value classes

`class Meter(val n: Int) extends AnyVal` is represented by its underlying
value; methods become `name$extension` statics, and the value is boxed as a
real `Meter` where a reference is needed (`Any`, a universal trait, a type
argument, an array element, a lambda parameter). `equals` / `hashCode` come
from the underlying value, and pattern matching, `classOf` and
`asInstanceOf` see the boxed class. As in nsc's `eraseDerivedValueClassRef`,
a generic value class instantiated at a primitive (`Wrap[Int]` for
`class Wrap[A](val a: A) extends AnyVal`) stays boxed as `java.lang.Integer`.
Both entry points nsc publishes exist: a static `name$extension` on the class
and an instance `name$extension` on the companion `Meter$`, which the
companion's pickle declares (including those of default getters), so a
scalac client links. The body is on the class's static and the companion
forwards to it; nsc does the reverse.
`crates/cli/tests/value_class_abi.rs` checks every pairing of the two
compilers.

## Compiler flags

- `-Xsource:3` / `-Xsource:3-cross` enable the 2.13 cross-building spellings:
  `?` wildcards (accepted without the flag too, as scalac does), `&`
  intersections, `case C(xs*)`, `f(xs*)`, `import p.*` and `import p.{a as b}`.
- `-Xsource-features`: the feature names and groups are parsed and validated;
  `case-apply-copy-access` and `unicode-escapes-raw` are implemented and
  `infer-override` is partial (see [not-implemented.md](not-implemented.md)).
- `-Xasync` with scala-async 1.0.1 ([async.md](async.md)).
- `-Ykind-projector`: kind-projector's `*` placeholders and `λ[α => …]` /
  `Lambda[…]` type lambdas, desugared to structural type lambdas. The
  term-level `λ[F ~> G](f)` is not supported.
- `-language:<feature>`, `-feature`, `-deprecation`, `-Xfatal-warnings`,
  `-nowarn`, `-no-specialization`, `--diagnostics=scalac`.

Run `scala-rs --help` for the full list.

## Macros

Def macros are expanded by loading the implementation's class files on a JVM
through a resident engine (`crates/typer/java/ScalaRsMacroEngine.java`); the
implementation must come from a previous run, as in nsc. Quasiquotes
(`q` / `tq` / `pq` / `cq`, in expressions and in patterns), `reify`,
`Liftable` holes for the standard instances, macro bundles and a subset of
the `Context` API are supported. Shapes outside the subset are reported by
name. See [macros.md](macros.md).
