# def Macro Design Notes

Design and current state of Scala 2.13 **def macros** (`def f = macro impl`) in scala-rs.
The original target was compiling slick without modification, and slick uses only two macros.

- `slick/lifted/ShapedValue.scala`
  `def mapTo[R <: Product with Serializable](implicit rCT: ClassTag[R]): MappedProjection[R] = macro ShapedValue.mapToImpl[R, U]`
- `slick/lifted/TableQuery.scala`
  `def apply[E <: AbstractTable[_]]: TableQuery[E] = macro TableQueryMacroImpl.apply[E]`

scala-rs expands a def macro by running its implementation for real on a JVM (§2). The bridge is
`crates/typer/java/ScalaRsMacroEngine.java`, driven from `crates/typer/src/expand*.rs`.
Quasiquotes and `reify`, which nsc implements inside the compiler, are built into scala-rs itself
(§6, §7.26, §7.27). slick's two macros expand, including `mapTo` against classes the same run is
compiling (§7.25); whitebox macros are supported (§7.30), and macro bundles expand (see
[`refined.md`](refined.md)).

Sections 0–6 are the design. Section 7 describes the implementation piece by piece, in the order it
was built; later pieces sometimes replace earlier ones, and say so. **Section numbers are stable
identifiers**: compiler diagnostics and code comments cite them (for example "see docs/macros.md
§6.2"), so a section that was removed leaves a gap instead of renumbering the ones after it.

## Table of contents

- 0. Summary
- 1. How nsc handles def macros
  - 1.1 The definition side
  - 1.2 The call side (expansion)
  - 1.3 Execution
  - 1.4 Signature rules
- 2. Choosing an execution model
  - 2.1 Option A: an interpreter over our own AST
  - 2.2 Option B: a JVM bridge (adopted)
  - 2.3 Validation with a prototype
  - 2.4 The cost of option B
  - 2.5 Intermediate options we rejected
- 3. The subset of the reflect API the engine implements
  - 3.1 Context
  - 3.2 The universe members `TableQueryMacroImpl.apply` touches
  - 3.3 The universe members `ShapedValue.mapToImpl` touches
- 4. Converting between our AST and reflect Trees
  - 4.1 Directions
  - 4.2 Wire format
  - 4.3 The limits of soundness
- 5. What has to survive in the classfile (separate compilation)
  - 5.1 A class the current run is compiling, as a type argument
- 6. Built-in (fast track) macros
  - 6.1 Quasiquotes and `reify` have no implementation in scala-reflect.jar
  - 6.2 So scala-rs reifies them itself
- 7. Implementation
  - 7.1 The quasiquote front end
  - 7.2 Holes plugged on the way to the reflect ABI
  - 7.4 Calling from the declaring class, and reification
  - 7.6 Macro implementation signatures and `import c.universe._`
  - 7.7 The remaining reification shapes
  - 7.8 `Liftable`, `symbolOf` / `weakTypeOf`, and diagnosing `reify`
  - 7.9 Quasiquoting definitions
  - 7.9a The three shapes that need fresh names
  - 7.10 `TypeTag` / `WeakTypeTag` materialization
  - 7.11 The engine: calling macro implementations
  - 7.12 `c.Expr[T](tree)` and `c.prefix`
  - 7.13 `Function` / `ValDef` in expansion results
  - 7.14 Nested `object`s and `<val>.type`
  - 7.15 Expanding `reify { … }`
  - 7.16 What compiling `ShapedValue.mapToImpl` needed
  - 7.17 Blocks, and members of static `object`s, in `reify`
  - 7.19 `val` and `def` definitions bound inside a `reify` body
  - 7.20 Reverse RPC: `c.typecheck`, `c.inferImplicitValue`, and the current run's symbols
  - 7.21 A type tag that carries type arguments, and the `Expr[Nothing]` nsc really passes
  - 7.22 The type arguments written on the macro implementation reference
  - 7.23 Structural transport
  - 7.24 Source symbol ownership
  - 7.25 `mapTo` against classes this run is compiling
  - 7.26 `reify { … }` over the typed body
  - 7.27 Quasiquotes in pattern position
  - 7.30 Whitebox macros

(7.3, 7.5, 7.18, 7.28 and 7.29 are gaps left by removed sections; 7.9a was the second of two
sections both numbered 7.10.)

---

## 0. Summary

- The execution model is the **JVM bridge**. We do not interpret macro implementations over our own
  AST.
- The rationale is that `scala.reflect.macros.blackbox.Context` has only 72 abstract members, and
  every one of them is an ordinary JVM interface method that passes `scala.reflect.api.*` values
  around, plus the fact that we can plug **`scala.reflect.runtime.universe` (the complete
  implementation bundled in scala-reflect.jar)** straight into `c.universe`. The latter is
  guaranteed at the type level
  (`scala.reflect.internal.SymbolTable extends scala.reflect.macros.Universe`,
  `scala.reflect.runtime.JavaUniverse extends scala.reflect.internal.SymbolTable`).
- The design was validated with a working prototype before it was built (§2.3): a macro
  implementation compiled by scalac, invoked through a Context built with Java's
  `java.lang.reflect.Proxy`, returned exactly the reflect Trees we expected for `reify`,
  **quasiquotes**, and `WeakTypeTag`.
- Quasiquotes and `reify` **cannot be expanded through the JVM bridge**. They have no implementation
  classfiles in scala-reflect.jar: they are **compiler-internal (fast track) macros of nsc** (§6).
  scala-rs implements them itself, desugaring into the same `internal.reificationSupport` calls nsc
  emits (§7.1–§7.9a for quasiquotes, §7.15 and §7.26 for `reify`, §7.27 for quasiquote patterns).
- The engine answers `c.typecheck`, `c.inferImplicitValue` and questions about classes the current
  run is compiling by asking the Rust typer back over the same pipe (§7.20, §7.25).
- Whatever cannot be expanded is a diagnostic that names the reason. A macro call is never silently
  accepted.

---

## 1. How nsc handles def macros

### 1.1 The definition side

```scala
def f(x: Int): Int = macro impl
```

- `macro` is a soft keyword that appears only on the right-hand side of a def. The RHS is not an
  expression but is restricted to **a reference to the macro implementation** (`Ident` / `Select`,
  or a `TypeApply` of either).
- After type checking, the symbol of a macro def gets the `MACRO` flag. Its value is `1L << 15`
  (confirmed in the bytecode of `scala.reflect.internal.HasFlags.isMacro`). Bit 15 lies in the
  **pickled flag region**, so it survives into the classfile and later runs read it back.
- A macro def **may not omit its return type** (it cannot be inferred from the implementation's
  return type).
- A macro def **leaves no bytecode**. Every call site disappears into an expansion, so no actual
  method is needed.
- So that expansion can happen from another compilation unit, nsc bakes the "macro def → macro
  implementation" correspondence into the classfile as a
  **`@scala.reflect.macros.internal.macroImpl(...)` annotation in the pickle**. That annotation
  class lives in **scala-library.jar** (not in reflect), so a classfile containing a macro def only
  references classes that are on the user's runtime classpath.

  Its contents are the six fields of `Macros$MacroImplBinding`; the key names in the pickle have
  been confirmed:

  | Key | Contents |
  | --- | --- |
  | `macroEngine` | Fixed at `"v7.0 (implemented in Scala 2.11.0-M8)"`. A mismatch is an expansion error |
  | `isBundle` | Whether the implementation is a method of a "bundle class" (`class B(val c: Context)`) |
  | `isBlackbox` | Whether the type of the implementation's `c` is blackbox or whitebox. **The only way the expansion side learns which box it is** |
  | `className` | Binary name of the class holding the implementation. For an object, with a trailing `$` (`pkg.Foo$`) |
  | `methodName` | Name of the implementation method |
  | `signature` | `List[List[Fingerprint]]` — how to build the arguments (table below) |

  `Fingerprint` is a value class over `Int`:

  | Value | Meaning |
  | --- | --- |
  | `Other` = -1 | Pass through as is (the `Context` itself, and so on) |
  | `LiftedTyped` = -2 | Wrap the argument Tree in a `c.Expr[T]` |
  | `LiftedUntyped` = -3 | Pass the raw `c.Tree` |
  | `Tagged(i)` ≥ 0 | Pass the `WeakTypeTag` of the macro def's i-th type parameter |

  The type arguments (the `[A, B]` of `macro Impl.impl[A, B]`) are not a named field; they are
  recovered from the `TypeApply` structure of the annotation tree.

### 1.2 The call side (expansion)

- Expansion happens **inside the typer phase**. There is no dedicated macro phase. The unit of
  expansion is the **macro application**, i.e. the **outermost** node including any `Apply` /
  `TypeApply` (`M.f(1)`, not just `M.f`).
- The resulting Tree is **always re-typechecked** at the call site. nsc does not take the tree a
  macro returned on trust.
- **blackbox**: the expansion result is **explicitly overwritten** with the declared type via
  `Typed(expanded, TypeTree(innerPt))` and typechecked **exactly once** (`innerPt` is the declared
  return type with the call site's type arguments substituted in). Any more precise type the
  expansion result had is **discarded**. Every restriction on blackbox macros — cannot narrow the
  return type, cannot produce structural types, cannot drive type inference, cannot be an extractor
  macro — falls out of that single line of ascription.
- **whitebox**: type checking is performed **three times**. `#0` runs against `WildcardType` (with
  implicits disabled) to learn the expansion result's actual type and to instantiate undetermined
  type parameters via `inferExprInstance`; `#1` runs against `innerPt` and `#2` against `outerPt`.
  The narrowed type **is retained**.
- For the case where type arguments are still undetermined there is a **delay mechanism**
  (`delayed` / `undetparams` / `hasPendingMacroExpansions`): expansion is deferred and resumed once
  inference has made progress.
- Both of slick's macros are blackbox. Whitebox macros are supported too (§7.30).

### 1.3 Execution

- nsc **really executes** macro implementations on the JVM. It loads the implementation class with a
  dedicated class loader (`-Ymacro-classpath`, or the compile-time classpath;
  `ScalaClassLoader.URLClassLoader`, cached on file modification time) and calls it via Java
  reflection.

```
classLoader = URLClassLoader(-Ymacro-classpath | -classpath)
receiver    = isBundle ? ctor(Context).newInstance(c)
                       : ReflectionUtils.staticSingletonInstance(className)   // MODULE$
method      = Class.forName(className).getMethods.filter(_.getName == methodName).head
                                                  // overloading is already forbidden at the definition site
invoke      = isBundle ? method.invoke(receiver, others…)
                       : method.invoke(receiver, (c +: others)…)
others      = an Object[] assembled by interpreting `signature` through Fingerprint
```

- Therefore **the macro implementation must already be compiled before the compilation run in which
  the expansion happens**. This is not a special check; `Class.forName` simply fails. nsc's wording:
  "macro implementation not found ... (the most common reason for that is that you cannot use macro
  implementations in the same compilation run that defines them)".
  Putting the implementation and the def in the same **file** is fine (this is slick's shape).
  What you cannot do is "define an implementation and expand it on the spot" within the same **run**.
- Argument passing: the `Context` first, then one `c.Expr[T]` (or a raw `c.Tree`) per macro def
  argument, then a trailing `c.WeakTypeTag[T]` per type parameter.
- **fast track**: `reify` / quasiquotes / `materializeClassTag` / `materializeTypeTag` /
  `StringContext.f` and friends never go through the classloader; they short-circuit to
  compiler-internal implementations. That is the crux of §6.

### 1.4 Signature rules

For a macro def

```scala
def f[T1, …](a1: A1, …)(b1: B1, …): R = macro impl[T1, …]
```

the implementation must have the shape

```scala
def impl[T1, …](c: Context)(a1: c.Expr[A1], …)(b1: c.Expr[B1], …)
                (implicit t1: c.WeakTypeTag[T1], …): c.Expr[R]
```

The **bundle** form is allowed instead of an `object`.

```scala
class Bundle(val c: blackbox.Context) {
  def impl[T1, …](a1: c.Expr[A1], …): c.Expr[R]   // the Context is on the constructor
}
```

The rules (exactly the checks in `DefaultMacroCompiler$MacroImplRefCompiler`):

- `c` is the first parameter of the first parameter list (object form) / the sole constructor
  parameter (bundle form). **Its static type decides blackbox vs. whitebox.**
- Each value parameter is raised one meta level: `Ai` ⇒ `c.Expr[Ai]`. Since 2.11 a raw `c.Tree` is
  also allowed.
- The return type likewise: `R` ⇒ `c.Expr[R]`, or `c.Tree` (slick's `mapToImpl` returns a `Tree`).
- **Parameter names must match** those on the def side, and vararg-ness must match position by
  position.
- Type parameters correspond one to one, and a trailing implicit list may carry
  `c.WeakTypeTag[Ti]` (optional; omitting it just means no tag arrives). **No other implicit
  parameters are permitted.**
- The implementation must be `public` and **not overloaded** (because resolution at run time is
  `getMethods.filter(name).head`).
- If the shape of the reference differs, `macro implementation reference has wrong shape` is
  reported.

---

## 2. Choosing an execution model

### 2.1 Option A: an interpreter over our own AST

Parse the Scala source of the macro implementation with scala-rs and execute that AST with an
interpreter on the Rust side. The `scala.reflect` API would be reimplemented in Rust types.

- Upside: no JVM needed, no extra compile-time dependency.
- Downsides (fatal):
  - `scala.reflect.api` is enormous: Tree / Type / Symbol / Name / Constant / Mirror / Position /
    Liftable / Unliftable / TypeTag / Printers / ReificationSupport, and more. Even for just slick's
    two macros the surface actually touched is as wide as §3 shows.
  - An interpreter means building, from scratch, a language implementation that can execute a subset
    of Scala — closures, pattern matching, implicits, collections — separately from the compiler
    proper.
  - And **there is no guarantee the reimplementation matches the real thing**. For a macro, "emitting
    the same tree as the real implementation" is everything, so any divergence here means slick does
    not work.

**Not taken.** Not only is the effort large, we would have no grounds for believing it correct.

### 2.2 Option B: a JVM bridge (adopted)

Run macro implementations for real on the JVM. We supply the `Context`, and plug
**scala-reflect.jar's `scala.reflect.runtime.universe`** into `c.universe`.

```
scala-rs (Rust)                      macro engine (JVM)
──────────────                       ──────────────────
find the call site
  ↓ serialize the expansion request
    (impl class/method, argument
     Trees, type arguments)
                        ──────→     build a Context
                                       universe = scala.reflect.runtime.universe
                                       mirror   = runtimeMirror(macro classpath)
                                     build the argument Trees as universe Trees
                                     invoke the implementation method reflectively
                                       ↑ reify / quasiquotes / WeakTypeTag run
                                         as the real implementations
                                     serialize the returned Tree
                        ←──────
  convert the Tree into a scala-rs AST
  re-typecheck it at the call site
```

Two facts settle it.

1. `blackbox.Context` has only **72** abstract members, and every one is an ordinary interface method
   passing `scala.reflect.api.*` values. That is an amount of implementation we can write.
   (Confirmed with `javap -cp scala-reflect.jar scala.reflect.macros.blackbox.Context` plus its 11
   parent traits.)
2. **A complete implementation of the `scala.reflect.macros.Universe` we need for `c.universe`
   already exists.**

```
scala.reflect.internal.SymbolTable  extends scala.reflect.macros.Universe
scala.reflect.runtime.JavaUniverse  extends scala.reflect.internal.SymbolTable
scala.reflect.runtime.universe: scala.reflect.api.JavaUniverse (= a JavaUniverse value)
```

nsc plugs itself (`Global`) into `c.universe`. We plug in the runtime universe instead. For the
purpose of merely building `Tree`s, the two are just different implementations of the same interface.

### 2.3 Validation with a prototype

We wrote an approximately 180-line probe that does nothing but "build a `blackbox.Context` with
Java's `java.lang.reflect.Proxy` and return the runtime universe from `universe()`", and used it to
actually invoke macro implementations compiled by scalac. Thanks to JDK 17's
`InvocationHandler.invokeDefault`, the traits' default implementations (`weakTypeOf` and friends) run
for real. The probe grew into the production engine (§7.11); the standalone prototype is no longer
in the tree (see git history).

The macro implementations tested, and the results:

| Pattern | Implementation | Tree obtained |
| --- | --- | --- |
| Bare Tree construction | `c.Expr[Int](Literal(Constant(42)))` | `Literal(Constant(42))` |
| `reify` (= the shape of slick's `TableQueryMacroImpl`) | `c.universe.reify { Helper.hello(7) }` | `Apply(Select(Ident(Helper), TermName("hello")), List(Literal(Constant(7))))` |
| **quasiquote** (= the shape of slick's `mapToImpl`) | `c.Expr[Int](q"${x.tree} + 1")` | `Apply(Select(Literal(Constant(41)), TermName("$plus")), List(Literal(Constant(1))))` |
| `WeakTypeTag` | `c.Expr[String](Literal(Constant(t.tpe.toString)))` | `Literal(Constant("String"))` |

So **both reify and quasiquotes work as is on the runtime universe, provided they have been
compiled**. That is the strongest empirical support for option B.
(What is running here are the `Syntactic*` / `TreeCreator` calls that scalac has already desugared
and compiled. **Desugaring them from source is a separate problem**, and that is §6.)

Operational notes this probe turned up:

- The `TreeCreator` that `reify` generates looks symbols up with `mirror.staticModule("…")`.
  Therefore **the engine's JVM classpath must also carry the classes the compiled code refers to**.
  Loading only the macro implementation gives `ScalaReflectionException: object Helper not found`.
- `c.Expr[T](tree)` can be implemented by expanding to
  `universe.Expr(mirror, FixedMirrorTreeCreator(mirror, tree))(tag)`
  (`scala.reflect.internal.StdCreators$FixedMirrorTreeCreator`).

### 2.4 The cost of option B

- **Two new compile-time dependencies**: a JVM and `scala-reflect.jar`. Today scala-rs treats even
  scala-library.jar as optional (`--no-scala-library` gives a private runtime). Macros become "a
  feature that only works when the jar is present". When the jar is missing we **do not silently
  accept the program; we emit a diagnostic**.
- The engine has to be written in **Java**, not Rust (implementing Scala traits from Java). The build
  then needs `javac`. Whether to ship a prebuilt engine or run `javac` on first use is a separate
  decision.
- We have to fix an inter-process wire format (§4.2).

### 2.5 Intermediate options we rejected

- **Calling scala-compiler.jar directly**: delegating just macro expansion to nsc. That is no
  different from "calling scalac", and it would defeat the point of scala-rs being a Scala compiler.
  It would also be dishonest as a benchmark. Not taken.
- **Receiving the expansion result as a source string and re-reading it with the scala-rs parser**:
  not adopted as the wire format (§4.2): `showCode` drops symbols, so on its own it is unsound (it
  confuses distinct symbols that share a name). See the limits in §4.3.

---

## 3. The subset of the reflect API the engine implements

What follows was measured by reading the **compiled slick 3.4.1** (`slick_2.13-3.4.1.jar`) with
`javap -c -p`. In 3.4.1, `mapToImpl` lives in `Shape.scala` and `TableQueryMacroImpl` in
`Query.scala`, so the file layout differs from the `scala-2/slick/lifted/` arrangement of 3.5.x.

The surface slick's two macros actually touch in the bytecode. Note that **the only thing we
implement on the engine side is the `Context`**; the members on the `universe` side are the real ones
from scala-reflect.jar and run as is. So §3.2 and §3.3 are a checklist for "is the engine broken?",
not a list of "things to rewrite in Rust".

### 3.1 Context

The engine's `Context` is a `java.lang.reflect.Proxy` that implements `whitebox.Context` (which
extends `blackbox.Context`, so one proxy serves both kinds; §7.30). The members it answers:

| Member | Implementation |
| --- | --- |
| `universe` | the runtime universe |
| `mirror` | `universe.runtimeMirror(<macro class loader>)` |
| `Expr` / `Expr(tree)(tag)` / `WeakTypeTag` / `TypeTag` / `TermName` / `TypeName` / `literal` | the universe's own companions; `Expr(tree)` as in §2.3 |
| `weakTypeOf` / `typeOf` / `symbolOf` and the other trait defaults | the traits' default implementations run |
| `prefix` / `macroApplication` | trees sent from the call site (§7.12, §7.25) |
| `enclosingPosition` | the macro application's position (§7.23) |
| `abort(pos, msg)` | throws; the Rust side turns it into an error diagnostic |
| `freshName` | a monotonically increasing counter |
| `typecheck` (TERM and TYPE mode), `untypecheck` / `resetLocalAttrs`, `parse` | reverse RPC to the Rust typer and parser (§7.20, §7.23) |
| `inferImplicitValue`, `openImplicits` | reverse RPC to the Rust typer's implicit search (§7.20) |
| `openMacros` / `enclosingMacros` | the engine's own stack of active contexts |
| `internal` (`enclosingOwner`, `changeOwner`, attachments, …) | §7.24; anything else is forwarded to the universe's `internal` |
| `settings` / `compilerSettings` | the compiler options, `-Xmacro-settings:` split out |
| `TypecheckException` | the real companion |

Every other member **fails explicitly** with
`UnsupportedOperationException("scala-rs macro engine: Context.<name> is not implemented")`, and the
Rust side puts that name in the diagnostic. `inferImplicitView` is one of them.

### 3.2 The universe members `TableQueryMacroImpl.apply` touches

`Function` / `ValDef` / `Modifiers` / `Flag.PARAM` / `TermName` / `Ident` (both the `Symbol` and the
`Name` overloads) / `Select` / `New` / `TypeTree(tpe)` / `Apply` / `EmptyTree` /
`termNames.CONSTRUCTOR` / `typeOf[Tag]` / `rootMirror` / the `TreeCreator` and `TypeCreator` of
`reify` (`internal.reificationSupport.mkIdent` / `mkTypeTree`, `Mirror.staticModule` /
`staticClass`).
On the Symbol / Type side, **only** `WeakTypeTag.tpe` and `Type.typeSymbol`.

### 3.3 The universe members `ShapedValue.mapToImpl` touches

The body is almost entirely quasiquotes (`q` / `tq` / `pq` / `cq`). After desugaring there are 209
`internal.reificationSupport.Syntactic*` call sites:
`SyntacticSelectTerm`(60) / `SyntacticTermIdent`(35) / `SyntacticSelectType`(14) /
`SyntacticFunctionType`(12) / `SyntacticValDef`(11) / `SyntacticApplied`(11) /
`SyntacticAppliedType`(10) / `SyntacticFunction`(8) / `SyntacticTypeIdent`(7) /
`SyntacticEmptyTypeTree`(6) / `SyntacticNew`(4) / `SyntacticDefDef`(4) /
`SyntacticBlock`(3) / `SyntacticPartialFunction`(3) / `SyntacticSingletonType` /
`SyntacticExistentialType` / `SyntacticAssign` / `FlagsRepr` / `freshTermName` /
`freshTypeName` / `mkRefTree`.

The Tree constructors used directly are `TermName`(107) / `TypeName`(37) / `Typed`(15) /
`Modifiers`(13) / `Bind`(5) / `CaseDef`(4) / `EmptyTree`(22) / `noSelfType` /
`NoSymbol` / `This` / `Super` / `TypeDef` / `TypeBoundsTree` / `Constant` /
`symbolOf` / `Liftable` (`liftTypeTag` about 26 times, among others).

On the Symbol / Type side: `WeakTypeTag.tpe` / `Type.typeSymbol` / `TypeSymbol.isClass` /
`.asClass.isCaseClass` / `.fullName` / `.name.toTermName` / `.companion` /
`Symbol.info` / `Type.decls.collect` / `Type.member(Name)`.
In other words, it **enumerates the fields of a case class**. There is no implicit search and no
annotation reading.

**Important**: everything above is handled by the real implementations in scala-reflect.jar. What we
provide is the Context and the Tree input/output conversion, and nothing else.

---

## 4. Converting between our AST and reflect Trees

### 4.1 Directions

- **Input (Rust → JVM)**: the argument expressions of the macro call. We build reflect Trees from
  typechecked scala-rs ASTs. Slick's two macros **barely look inside** the argument Trees
  (`mapToImpl` uses `c.prefix` and the type arguments; `TableQueryMacroImpl` uses only the type
  arguments), but general macros do, so the input side carries typed trees (§7.23, §7.25).
- **Output (JVM → Rust)**: the expansion result Tree. Here we do have to read **everything**.

### 4.2 Wire format

The `showRaw` form (`Apply(Select(Ident(Helper), TermName("hello")), List(Literal(Constant(7))))`)
comes out directly, as the prototype confirmed, but re-parsing it on the Rust side would be a lot of
work and its escaping rules are murky. The engine therefore serializes to **S-expressions**, one
message per line on the pipe (§7.11). Both ends parse them with a few dozen lines of code.

```
→ (expand "EgImpl$" "plusImpl" (argss (args (arg expr <tree> (ty "scala.Int")))) (tags))
← (ok (t "Apply" (s0) (t "Select" (s0) (t "Literal" (s0) (c "Int" "41")) (n term "$plus"))
        (l (t "Literal" (s0) (c "Int" "1")))))
```

Every tree node is `(t "<productPrefix>" <symbol> <elements>…)`; the engine lays out
`productElement` generically and does not know the node kinds. A symbol carries a fully qualified
name when it is static; a class the current run is compiling travels as its scala-rs identity
(`(src <id>)`, §7.25). The Rust side resolves by symbol where it has one and by name otherwise, and
an unknown node kind is always a diagnostic that names it. The same channel carries the engine's
questions back to the typer (`(q …)` / `(a …)`, §7.20).

Two conventions matter for what the engine keeps:

- **A refinement travels under a placeholder label chosen by content**
  (`(refined "<macro-type-N>" (parents …) (members …))`): equal refinements get the same label for
  the whole run, and the label always names the same scala-rs type (`Typer::refined_label`). The
  engine can therefore keep what it built for a type that contains one -- every field of a labelled
  `HList` does -- in its per-text type cache, and remembers each refinement's label for the run to
  write it back (`refinedLabels`).
- **A by-name argument goes as the expression itself**, as it stands in nsc's typed tree. The typer
  wraps it in a thunk (`Tree::byname_thunk`) that only exists for lowering; sent as a `() => e`
  function, it came back from `c.untypecheck` as a literal that no longer fits the `=> T`
  parameter of an overloaded method (circe's `DecodingFailure.apply(…, c.history)` inside every
  `Lazy` derivation of a sealed trait).

### 4.3 The limits of soundness

- The expansion result Tree points at **symbols of the runtime universe on the JVM side**. Those are
  distinct from the symbols in the Rust-side SymbolTable. Matching them by the fully qualified name
  is the bridge, but **symbols with no fully qualified name** — local variables, type
  parameters, anonymous function parameters — can only be carried by name. This can break variable
  capture (hygiene).
  Since nsc's def macros are not hygienic either (the culture is to work around it with
  `freshName`), "as unsound as the real thing" is the bar. Source symbols do keep their identity
  across the protocol (§7.24).
- If a **Tree with an embedded Type**, such as `TypeTree(tpe)`, comes back, the Type has to be
  serialized the same way and turned back into a Rust-side `Type`. Both slick macros use this
  (`TableQueryMacroImpl` produces `TypeTree(e.tpe)`).
- Re-reading a `showCode` string with the scala-rs parser drops the symbols above and is therefore
  **unsound in general**. Keep it to debug output.
- The call site's receiver and arguments go to the engine **typed**, and when an implementation
  returns one of them unchanged scala-rs splices its own typed tree back instead of typing a rebuilt
  copy again (§7.25). A tree the implementation built or changed is rebuilt from its shape and typed
  at the call site.

---

## 5. What has to survive in the classfile (separate compilation)

Since macro defs are expanded from other compilation units, the `ScalaSignature` must record "this
method is a macro, and its implementation is X.y".

- nsc: bakes `@macroImpl(tree)` (the six fields of §1.1) into the pickle's `SYMANNOT` and sets the
  `MACRO` flag (`1L << 15`). The body of a macro def is `EmptyTree`, and **no JVM method is emitted**
  (which is why macros cannot be called from Java). To catch leaks, RefChecks has a
  `"macro has not been expanded"` check.
- scala-rs, reading: `scala_rs_pickle::sym` decodes the `SYMANNOT` holding
  `@scala.reflect.macros.internal.macroImpl` into `Member::macro_impl`, and
  `PickleSupply::install_pickled_macro` installs the declaration with that binding. So a macro def
  **in a published jar** -- slick's `TableQuery.apply[E]`, `ShapedValue.mapTo[R]` -- is a member
  with its real type, and its call sites go through the same expansion-or-diagnose path as a
  source-level one. The implementation in that case is *already compiled*, so §2.3's finding applies
  directly: `reify` and quasiquotes run as themselves.

  Two fields of the annotation are the whole binding: `className` / `methodName`, and `signature`,
  which is nsc's per-parameter `Fingerprint` list. The encoding, confirmed against every macro in
  `slick_2.13-3.4.1.jar`: the first clause is always the implementation's `(c: Context)`; `-1` is an
  ordinary value, `-2` a `c.Expr[T]`, `-3` a `c.Tree`, and a non-negative value a `WeakTypeTag` for
  the implementation's type parameter at that position. The type arguments written on the
  implementation reference are the `TypeApply` nsc wraps around the payload (§7.22). The eager flat
  pickle reader skips `MACRO` members, so only the full supplier installs them (§7.23).
- scala-rs, writing: `crates/backend/src/pickle.rs` gives a source macro def the `MACRO` flag and
  nsc's `@macroImpl` annotation, including the parameter fingerprints and the reference's type
  arguments, so a macro def scala-rs compiles can be expanded from a later run.
- **Macro defs emit no method body** (`crates/backend/src/gen.rs`).

### 5.1 A class the current run is compiling, as a type argument

A macro implementation is invoked through the JVM bridge, and the bridge builds a `WeakTypeTag`
inside `scala.reflect.runtime`'s universe, whose mirror resolves a class **by name against the macro
classpath**. A type argument that is a class *this run is compiling* has no class file there, so
`mirror.staticClass` can never find it. gitbucket's `lazy val Issues = TableQuery[Issues]` is exactly
that shape -- the table class is declared a few lines from the call.

Nor can scala-rs send a **snapshot** of such a class with the request. While
`lazy val DeployKeys = TableQuery[DeployKeys]` is being typed, the members of `class DeployKeys` may
still be un-inferred, because each is a `val` whose type comes from typing its right-hand side; there
is no instant at which a complete, truthful description exists. Anything richer than the class's
identity would be a guess, and an implementation that acts on a guess builds a tree from a class it
half understands.

So a current-run class travels as its **identity**, and the engine asks about it only when the
implementation does: its symbol's info is a lazy type that completes over the reverse channel
(§7.20), forcing exactly the signatures the typer would have forced, in nsc's shape (§7.25). When the
expansion mentions the class again -- as `TypeTree(e.tpe)`, which is what `TableQueryMacroImpl` does
twice -- the tree that comes back carries the `Type` the typer already had, not a path resolved again
by name. That matters beyond convenience: gitbucket's table classes are nested in traits, and
`gitbucket.core.model.DeployKeyComponent.DeployKeys` is not a path any scope at the call site can
resolve.

---

## 6. Built-in (fast track) macros

Running an expansion is solved by §2. For quasiquotes and `reify` it is not enough: scala-rs has to
compile the *source* of a macro implementation that uses them, and that turns on one fact.

### 6.1 Quasiquotes and `reify` have no implementation in scala-reflect.jar

The constant pool of `scala.tools.reflect.FastTrack` contains these names verbatim (confirmed with
`unzip -p scala-compiler.jar 'scala/tools/reflect/FastTrack.class' | strings`):

```
QuasiquoteClass_api_apply    QuasiquoteClass_api_unapply
ApiUniverseReify
materializeClassTag   materializeTypeTag   materializeWeakTypeTag
StringContext_f   StringContext_s   StringContext_raw
```

And **there is not a single pickled `@macroImpl` binding inside scala-reflect.jar** (a string search
for `macroEngine` gives zero hits). The declaration of `Universe.reify` is `= macro ???`.

In other words:

> **Quasiquotes (`q"…"` / `tq"…"` / `pq"…"` / `cq"…"`) and `reify` have no implementation classfiles
> in scala-reflect.jar. The real thing lives inside scala-compiler.jar, and nsc short-circuits to the
> built-in implementation without going through the classloader (fast track).**

So the JVM bridge cannot be used for these: there is no implementation class to load. They are not
something that comes for free once you have a macro expander.

### 6.2 So scala-rs reifies them itself

The shape of what has to be built is clear. All nsc's quasiquote macros do is **"parse the
interpolated string as Scala and desugar it into a sequence of
`internal.reificationSupport.Syntactic*` calls"** (the bytecode measurements in §3.3 back this up:
the body of `mapToImpl` desugars into 209 `Syntactic*` call sites).

So the work on the scala-rs side is:

1. **Parse the contents of `q"…"` as Scala, in a form that permits holes** (`$x` / `${…}`), with the
   scala-rs parser (§7.1).
2. Lower the parse result into an AST of `Syntactic*` calls
   (`SyntacticSelectTerm` / `SyntacticApplied` / `SyntacticValDef` / `SyntacticDefDef` /
   `SyntacticNew` / `SyntacticFunction` / `SyntacticBlock` / `FlagsRepr` / …) and type it as an
   ordinary expression (§7.4–§7.9a). Pattern quasiquotes lower to the universe's extractors instead
   (§7.27).
3. Generate code for that AST against the scala-reflect ABI, like any other library call (§7.2,
   §7.4).

That gives a **compiled `mapToImpl`**, and the engine runs it.

`reify` likewise needs a built-in that desugars the reified block into universe Tree construction
calls inside a generated `TreeCreator` (§7.15, §7.26), and `TypeTag` materialization one that
generates a `TypeCreator` (§7.10).

---

## 7. Implementation

Each subsection describes one piece of the implementation, in the order it was built. A later piece
sometimes replaces an earlier one; where it does, the earlier section says so and points forward.

### 7.1 The quasiquote front end (`crates/typer/src/quasiquote.rs`)

The front end recognizes `q"…"` / `tq"…"` / `pq"…"` / `cq"…"`. (Before it existed the diagnostic was
the **incorrect** `value q is not a member of StringContext`: `q` is a member of
`Quasiquotes.Quasiquote`.)

- The contents of the interpolated string are **reconstructed with the holes
  (`$x` / `${…}` / `..$xs` / `...$xss`) replaced by placeholder names, and actually parsed by the
  scala-rs parser**. Since `..` / `...` appear at the end of the preceding part, the rank is stripped
  from there.
- If it does not parse: `unimplemented syntax: quasiquote q"..." (reason)`.
- If it parses, it is reified (§7.4 onward). If it parses but cannot be reified where it stands
  (for instance with no universe in scope), the diagnostic is
  `macro expansion is not implemented: cannot expand quasiquote q"..."`.
- **We do not hijack user-defined `q` interpolators.** We first try to type it as an ordinary custom
  interpolator, and only report it as a quasiquote when that fails (the fixture `quasi.scala`
  verifies this all the way to run time).

### 7.2 Holes plugged on the way to the reflect ABI

Being able to expand `q"…"` is pointless if scala-rs cannot typecheck what it lowers to (`c.universe`
/ the runtime universe). These are the general fixes that needed; none of them is
reflect-specific.

1. **Nested classes referred to by the pickle.** The pickle writes package separators and class
   separators identically as dots, e.g. `scala.reflect.api.Names.TermNameExtractor`. The actual file
   is `scala/reflect/api/Names$TermNameExtractor.class`, and moreover **nested classfiles have no
   `ScalaSignature`** (the pickle is stored wholesale in the top-level class's classfile).
   `scala_rs_pickle::sym::pickle_files_for` generates candidate files right to left and resolves both.
2. **Traits with no parents in the bytecode.** `scala.reflect.api.Universe` is an *abstract class*,
   so the classfile of `trait JavaUniverse extends Universe` has `interfaces: 0` and the inheritance
   relation exists only in the pickle. `erased_desc` fills in the pickle's parents, but only for
   classes whose classfile declares no parent at all (filling in unconditionally makes the erased
   descriptor of `Map#map` ambiguous).
3. **Abstract type members.** Declarations like `type Tree >: Null <: TreeApi` are the vocabulary of
   the reflect API itself; they are not classes, so `ensure_class` cannot resolve them.
   `PickleSupply::abstract_type_member` introduces them as `TypeMember` symbols. For the case where
   they are written **unqualified** from inside the class, as with `Constant`, we search the
   receiver's linearization and its **enclosing class** as well (`self_type_member`).
4. **Inserting `apply` on a parameterless `def`.** `Literal(x)` against `def Literal: LiteralExtractor`
   means `Literal.apply(x)`. This is a general gap, not a reflect-specific one: `mk("a")` against
   `def mk: Box` did not work either (`insert_apply_on_nullary`).
5. **Code generation for package object members.** `scala.math.Pi` is a `val` in
   `scala/math/package$`, but the typer folds it into the package symbol. A package has no runtime
   value, so an `invokevirtual` was emitted with no receiver pushed, giving a **`VerifyError`**
   (an existing bug reproducible in `main` too). `load_package_object_receiver` pushes
   `<pkg>/package$.MODULE$`.
6. **`import <value>._`.** The `import c.universe._` / `import scala.reflect.runtime.universe._`
   shape. When the prefix is a value, the members of its **type** have to be brought in, and further,
   an unqualified `Literal` means `u.Literal`, so the typer rewrites it back to `Select(u, Literal)`
   (`term_import_prefixes` / `qualify_term_import`). Without this the backend uses `this` as the
   receiver and gets a `ClassCastException`.

### 7.4 Calling from the declaring class, and reification

**Code that builds Trees on `scala.reflect.runtime.universe` actually runs**, and on top of that
`q"…"` really gets desugared.

#### 1. Calling from the declaring class

We added `Symbol::declaring_class` / `declaring_is_interface` (`crates/typer/src/symbol.rs`).
Item 2 of §7.2 had made `pickle_supply::erased_desc` "fill in the pickle's parents for classes whose
classfile declares no parent", but **it did not report which class the descriptor it found was
declared in**. That became
`ErasedDecl { desc, declared_in, declared_by_interface, off_the_bytecode_path }`, and only when the
descriptor was found via `off_the_bytecode_path` (i.e. reachable only by following the pickle's
parents, invisible to the JVM) do we record the declaring class on the symbol. `gen.rs` uses it as
the invoke owner and `checkcast`s the receiver to it. **The bytecode of ordinary members reachable
from the receiver's own classfile does not change at all** (the existing fixtures pin all of that).

```
// What scala-rs now emits (same shape as nsc)
invokeinterface scala/reflect/api/Constants.Constant:()Lscala/reflect/api/Constants$ConstantExtractor;
// Before: invokeinterface scala/reflect/api/JavaUniverse.Constant() → NoSuchMethodError
```

That alone did not get `u.Literal(u.Constant(42))` through; four more holes were plugged on the way.
All are general gaps, not reflect-specific.

- **Nested class names collapsing into the enclosing class** (`pickle_supply::ensure_class`).
  `pickle_files_for` also offers "the classfile that contains the pickle" as a candidate, so
  `scala.reflect.api.Constants.Constant` (an abstract type member with no runtime entity) matched
  `scala/reflect/api/Constants` and resolved to **the enclosing trait itself**. `names_class` now
  keeps only candidates that end with the simple name in question.
- **Compound upper bounds being dropped** (`conv_upper_bound`). The reflect API is written in the
  form `type Select >: Null <: SelectApi with RefTree`; we could not convert `Refined` and dropped
  the whole upper bound. Since `Select <: Tree` was not derivable, there was nothing at all we could
  pass to `Syntactic*`.
- **Upper bounds being resolved in the receiver's vocabulary** (`abstract_type_member`). Bounds are
  written in the vocabulary of the *declaring* class (the `RefTree` in `Ident`'s upper bound is
  another abstract type member of the same `Trees`). We point `self_ty` at the declaring class for
  the duration of the conversion.
- **The default-argument getter convention.** When a default value does not read preceding
  parameters, scalac emits a **nullary** `$default$n`. The call side must match the getter's own
  arity (`default_getter_apply`). Without this, `SyntacticTermIdent` is not supplied.
- **Compound upper bounds not appearing in the base type sequence** (`SymbolTable::base_type_seq`).
  `lub(Ident, Literal)` came out as `AnyRef`, making `List(ident, literal)` a `List[AnyRef]`.

#### 2. Reification

`crates/typer/src/reify.rs`. It lowers the tree parsed by §7.1 into a call tree of
`<universe>.internal.reificationSupport.Syntactic*` and **typechecks and generates code for it as an
ordinary expression**. The universe is taken from the prefix of the term import recorded by
`import <universe>._` (`Check::universe_in_scope`).

The shapes of `q"…"` that can be lowered:

| Shape | Lowered to |
| --- | --- |
| Literal | `u.Literal(u.Constant(v))` |
| Name | `rs.SyntacticTermIdent(u.TermName("n"), false)` |
| `a.b` | `rs.SyntacticSelectTerm(<a>, u.TermName("b"))` |
| `f(a, b)` / `a.b(1)(2)` | `rs.SyntacticApplied(<f>, List(List(<a>, <b>)))` |
| `$x` | splice the argument expression in as is |
| `..$xs` | splice in as one whole argument-list section |
| `f()` | `Nil` (`List()` cannot resolve `A` from the expected type) |

**Shapes we cannot lower are always diagnosed** (the `unimplemented syntax: quasiquote q"..." (…)`
message names which shape it was). The rest of the shapes are §7.7, §7.9 and §7.9a.

Validation: `tests/fixtures/reify_qq.scala` is dual-run against the real scalac 2.13.16 and
**the output matches exactly** (`crates/cli/tests/reify.rs`). The failure cases are in
`tests/fixtures/reify_qq_bad.scala`.

### 7.6 Macro implementation signatures and `import c.universe._`

**If scala-reflect.jar is on the classpath, macro implementation sources compile.**
The substance was less about "path-dependent types" and more a general gap:
**lazy loading of jar classes was not reaching the type namespace or wildcard imports.**

| What was fixed | Where |
| --- | --- |
| **`import <value>._` not reaching inherited members.** Members of jar classes are lazily loaded from the pickle name by name. The `JavaUniverse` named by `import scala.reflect.runtime.universe._` inherits `TermName` / `Literal` / `Constant` / `termNames` **all from higher up the linearization** (`api.Names` / `Trees` / `Constants` / `StandardNames`), and since nobody had requested them, the import brought in **nothing**. The path route (`u.TermName`) worked because it goes through completion. Reified quasiquotes build `u.TermName(...)` explicitly, which is why this hole went unnoticed | `Check::expose_unqualified` → `supply_from_pickle_class` |
| **The type namespace.** The reflect API puts the same name in both namespaces (`val TermName` and `type TermName`). Resolving the value first puts the term in scope, and `expose_unqualified` then sees "already bound" and stops, so in `val n: TermName = TermName("f")` only the right-hand side went through and the left-hand side was `not found` | `Check::expose_unqualified_type` |
| **Type members of jar classes were not readable at all.** We only had completion for `def`s. `blackbox.Context` inherits `type Tree = universe.Tree` / `type Expr[T] = universe.Expr[T]` / `type WeakTypeTag[T] = …` from `scala.reflect.macros.Aliases`, and without them a macro implementation **cannot even write its own signature** | `PickleSupply::complete_type_member` / `install_type_alias` |
| **Type members through a refinement.** The `c` of slick's `mapToImpl` has the **refined type** `blackbox.Context { type PrefixType = ShapedValue[?, U] }`, and `c.Expr[…]` / `c.Tree` are projected out of it | the `Type::Refined` branch of `Check::project_from_prefix` |
| **The parents of an `import <value>._` were not loaded.** `universe_in_scope` identifies a universe by asking "does this prefix inherit `scala.reflect.api.Universe`?", but that parent list exists only in the pickle and nobody had read it yet. So every `q"…"` in a body that wrote `import c.universe._` came out as "cannot expand" | `PickleSupply::ensure_parents` |
| **The scope of a term import prefix.** The `u` of `import u._` is local to that method and does not exist in the next one. It was nonetheless used as a prefix, emitting a **`getfield` against another method's local** (`NoClassDefFoundError`). Worse, it evicted the enclosing import of the same owner, so after leaving the inner one there was no receiver at all | `Check::prefix_in_scope`; `remember_term_import_prefix` now appends instead of replacing |
| **We stopped installing an empty `Context` prelude.** We read the real one only when scala-reflect.jar is on the classpath. When it is absent we install the empty `Context` as before and **say so properly**: `value universe is not a member of Context` (`--scala-library` does not include scala-reflect.jar) | `prelude_reflect::want_context_stub` |

Validation (`crates/cli/tests/quasi.rs`):

- `tests/fixtures/qq_universe.scala` — run for real, and **the output matches the real scalac 2.13.16
  exactly**. Even `showRaw` matches, so we are building **the same tree**. `java -Xverify:all`.
- `tests/fixtures/qq_ctx.scala` — a macro implementation itself. **Both** scala-rs and the real
  scalac compile it, and the classfiles emitted load and verify on the JVM.
- `tests/fixtures/qq_ctx_bad.scala` — shapes that cannot be reified (type ascriptions, blocks, `tq`)
  are always diagnosed **by naming the shape**. Non-`Tree` holes are type errors too.
- The diagnostic for the empty `Context` without scala-reflect.jar is pinned as well.

### 7.7 The remaining reification shapes

**`tq"…"` / `pq"…"` / `cq"…"` and the remaining shapes of `q"…"` are lowered.** Every shape was read off the real scalac 2.13.16 with `-Ymacro-debug-lite` (which prints
the expansion nsc's own quasiquote macros emit), and `tests/fixtures/qr_forms.scala` compares against
the real scalac down to `showRaw` (run under `java -Xverify:all`; 56 lines match exactly).

#### Shapes that are lowered

| Shape | Lowered to |
| --- | --- |
| `tq"T"` | `rs.SyntacticTypeIdent(u.TypeName("T"))` |
| `tq"a.b.C"` | `rs.SyntacticSelectType(<a.b as a term>, u.TypeName("C"))` |
| `tq"F[A, B]"` | `rs.SyntacticAppliedType(<F>, List(<A>, <B>))` |
| `tq"A => B"` | `rs.SyntacticFunctionType(List(<A>), <B>)` |
| `tq"(A, B)"` | `rs.SyntacticTupleType(List(<A>, <B>))` |
| `tq"a.b.type"` | `rs.SyntacticSingletonType(<a.b>)` |
| `tq"A#B"` | `rs.SyntacticTypeProjection(<A>, u.TypeName("B"))` |
| `tq"A with B"` | `rs.SyntacticCompoundType(List(<A>, <B>), Nil)` |
| An empty type slot (the type of `val x = e`) | `rs.SyntacticEmptyTypeTree.apply()` |
| `q"x: T"` | `u.Typed(<x>, <T>)` |
| `q"f _"` | `u.Typed(<f>, rs.SyntacticFunction(Nil, u.EmptyTree))` |
| `q"f[T](a)"` | `SyntacticApplied` on top of `rs.SyntacticTypeApplied(<f>, List(<T>))` |
| `q"{ a; b }"` / `q"..$stats"` | `rs.SyntacticBlock(List(<a>, <b>))` |
| `q"val v: T = e"` | `rs.SyntacticValDef(u.Modifiers(rs.FlagsRepr(0L)), u.TermName("v"), <T>, <e>)` |
| `q"new C[T](a)(b)"` | `rs.SyntacticNew(Nil, List(rs.SyntacticApplied(<C[T]>, List(<a>, <b>))), u.noSelfType, Nil)` |
| `q"e match { … }"` | `rs.SyntacticMatch(<e>, List(<case>))` |
| `q"{ case p => e }"` | `rs.SyntacticPartialFunction(List(<case>))` |
| `q"(y: T) => e"` | `rs.SyntacticFunction(List(<param>), <e>)` |
| `q"this"` / `q"C.this"` | `u.This(u.TypeName(""))` / `u.This(u.TypeName("C"))` |
| `q"a.b = c"` | `rs.SyntacticAssign(<a.b>, <c>)` |
| `q"if (a) b else c"` | `u.If(<a>, <b>, <c>)` |
| `pq"_"` | `rs.SyntacticTermIdent(u.TermName("_"), false)` |
| `pq"x"` (lowercase initial) | `u.Bind(u.TermName("x"), rs.SyntacticTermIdent(u.TermName("_"), false))` |
| `pq"a.b.C(p)"` | `rs.SyntacticApplied(<a.b.C>, List(List(<p>)))` |
| `pq"x @ p"` / `pq"a \| b"` / `pq"_: T"` | `u.Bind` / `u.Alternative` / `u.Typed` |
| `cq"p if g => e"` | `u.CaseDef(<p>, <g>, <e>)` (`u.EmptyTree` when there is no guard) |
| Operator names | Encoded with `NameTransformer` (`q"a + b"` gives `u.TermName("$plus")`) |
| `q"$x.$n"` | A hole in name position splices the `TermName` straight in |

#### Recovering, from the original source string, distinctions the parser collapses

The scala-rs parser normalizes away several distinctions that nsc keeps. Reification
**carries the quasiquote's body text around** and uses it to tell them apart (`Reifier::src`).

- `A => B` becomes `AppliedTypeTree(Ident("Function1"), …)`, the same tree as a **written**
  `Function1[A, B]`. nsc makes the former `_root_.scala.Function1` and the latter a bare `Ident`.
  We decide by whether the text at the head span is `Function1`.
- `(a, b)` becomes `Apply(Ident("Tuple2"), …)`, the same tree as a written `Tuple2(a, b)`. nsc uses
  `SyntacticTuple` only for the former. Same test.
- `q"val v = e"` versus `q"{ val v = e }"`. The `{}` added by the wrapper and the author's own `{}`
  are indistinguishable after parsing, so we pass **whether the body starts with `{`**
  (`braced` in `unwrap_body`). The former is a bare `SyntacticValDef`, the latter a `SyntacticBlock`.

#### Shapes we cannot lower are diagnosed by name

We do not build shapes where the parser has discarded **the information itself**, so that whatever we
built would be "a tree nobody wrote". (Right-associative operators and `_` placeholders need fresh
names and are built by §7.9a; a `..$` mixed with ordinary arguments is built by §7.16; definitions by
§7.9.) `tests/fixtures/qr_forms_bad.scala` / `reify_qq_bad.scala` /
`qq_ctx_bad.scala` each pin the corresponding diagnostic.

| Shape | Diagnostic | Reason |
| --- | --- | --- |
| `q"if (a) b"` | an `if` without an `else` is not reified yet | The parser fills the `else` with `()`. nsc fills it with an empty block |
| `tq"=> T"` | a by-name type is not reified yet | nsc's own parser rejects it inside `tq` |
| `q"{ $x }"` | (no diagnostic; a known difference) | The parser collapses `{ e }` into `e`, so a lone hole comes out as `x` where nsc has `SyntacticBlock(List(x))`. The meaning is the same, the tree is not |

#### General holes fixed along the way

Reification merely happened to demand these; none of them is reflect-specific.

| What was fixed | Where |
| --- | --- |
| **Inserting `apply` on an overload set.** `val Ident: IdentExtractor` and `def Ident(name: String): Ident` form one overload set under the same name, and `Ident(TermName("x"))` matches neither: it is `Ident.apply(...)`. `Bind` / `This` / `New` have the same shape. slick's `TableQuery` macro implementation is written entirely out of this | the `Type::Overload` branch of `Check::insert_apply_on_nullary` |
| **A term selection being eaten by a type member of the same name.** The reflect API puts both `type Modifiers` and `def Modifiers(flags: FlagSet)`. Since jar members are lazily loaded name by name, once **the type member goes in first** (completing `NoMods` brings it in) the name is no longer "not found", so the term overload was never read and `u.Modifiers(flags)` resolved to a `TypeMember` of `<notype>` (`value apply is not a member of <notype>`). The mirror image of `expose_unqualified_type` in §7.6 | `Check::type_select` |
| **The `count` of `invokeinterface` not being a slot count.** `long` / `double` arguments take two slots. `reificationSupport.FlagsRepr(8192L)` was giving `VerifyError: Inconsistent args count operand in invokeinterface` | `Assembler::invokeinterface` / `count_param_slots` |
| **No erasure-adapting `checkcast` on abstract type member arguments.** `type TermName >: Null <: TermNameApi with Name` erases to `Names$TermNameApi` and `Name` to `Names$NameApi`, and the JVM does not know how the two relate. nsc emits a `checkcast` here | `adapt_type_member_arg` in `gen.rs` |
| **`NoMods` is declared on `Universe`.** `scala.reflect.api.Universe` is an abstract class, and `JavaUniverse`'s inheritance from it exists only in the pickle. `u.NoMods` became `invokevirtual scala/reflect/api/Universe.NoMods()` and failed verification. Reification uses `u.Modifiers(rs.FlagsRepr(0L))`, which builds the same value (`Modifiers(flags)` is `Modifiers(flags, typeNames.EMPTY, Nil)`) | `Reifier::mods` |

### 7.8 `Liftable`, `symbolOf` / `weakTypeOf`, and diagnosing `reify`

**Non-`Tree` holes lift**, so the `q"($rModule.tupled) : ($uTag => $rTag)"` family in
`ShapedValue.mapToImpl` compiles.

#### 1. `Liftable`

For non-`Tree` holes nsc searches for an implicit `Liftable[T]` and splices in
`Liftable.liftX[T](arg)` (`scala/reflect/api/StandardLiftables.scala`).
scala-rs **does not do implicit search**. It picks the standard instance from the type of the hole's
argument and **directly builds the same tree that instance would build**.

To learn the type, each argument is typed **speculatively** before reification (a clone is typed and
the diagnostics are rolled back; the same shape as `Check::probe_named_arg_types`. The tree at the
call site is only typed once). The classification is `Check::lift_for` and the tree construction is
`Reifier::lift` (`Lift` in `crates/typer/src/reify.rs`).

| Hole type | nsc | Tree scala-rs builds |
| --- | --- | --- |
| `Tree` (every type member of `Trees`) | `liftTree` = identity | spliced in as is |
| `Int` / `Long` / `Short` / `Byte` / `Char` / `Float` / `Double` / `Boolean` / `Unit` / `String` | `liftInt` & co. | `u.Literal(u.Constant(v))` |
| `Constant` | `liftConstant` | `u.Literal(c)` |
| `Type` (a type member of `Types`) | `liftType` | `rs.mkTypeTree(t)` |
| `WeakTypeTag` / `TypeTag` | `liftTypeTag` | `rs.mkTypeTree(tag.tpe)` |
| `Expr[T]` | `liftExpr` | `e.tree` |
| `Symbol` (a type member of `Symbols`) | **not** a Liftable (a special case for holes) | `rs.mkRefTree(u.EmptyTree, sym)` |
| `Name` (term position) | a special case for holes | `rs.SyntacticTermIdent(n, false)` |
| `Name` (type position) | as above | `rs.SyntacticTypeIdent(n)` |
| `Name` (pattern position) | as above | `u.Bind(n, rs.SyntacticTermIdent(u.TermName("_"), false))` |
| An element of `..$xs` that is any of the above | `xs.toList.map(v => liftX(v))` | the same shape (no `.toList` when it is already a `List`) |

The position dependence of `Name` comes from nsc's parser. The hole in `q"$n"` stands in identifier
position, so it becomes a term identifier under `q`, a type identifier under `tq`, and a variable
pattern under `pq`. Name **slots** (the `$n` in `q"$x.$n"`, or in `q"val $n = e"`) were already
spliced straight in.

`Symbol` alone is a special case for holes rather than a `Liftable`, so **nsc itself refuses it under
a `..$`** ("consider omitting the dots or providing an implicit instance of `Liftable[Symbol]`").
scala-rs refuses it the same way.

**Whatever we do not build, we diagnose by name**:
`a hole of type `X` is not lifted (the Liftable instances scala-rs builds are …)`.
We do not search for user-written `Liftable`s, so those get the same diagnostic (better than
silently building a different tree). What nsc has and scala-rs does not build are `liftList` /
`liftArray` / `liftMap` / `liftOption` / `liftEither` / `liftTuple*` / `liftScalaSymbol`, all of them
rank-0 hole shapes.

Validation: `tests/fixtures/lf2_lift.scala` is dual-run against the real scalac 2.13.16 and
**`showRaw` matches exactly** (since `showRaw` hides the type inside a `TypeTree`, we print `show`
alongside it). 29 lines. `WeakTypeTag` and `Expr` cannot be created at run time without a
materialiser, so `tests/fixtures/lf2_ctx.scala` compiles them **as a macro implementation**, checks
that both compilers accept it, and checks that the classfile loads and verifies under
`java -Xverify:all`. The failure cases are in `tests/fixtures/lf2_lift_bad.scala`.

#### 2. `symbolOf[T]` / `weakTypeOf[T]` / `typeOf[T]`

`def symbolOf[T](implicit tag: WeakTypeTag[T]): TypeSymbol` mentions its type
parameter **only in the implicit section** and not in the result type.
`pin_undetermined_tparams` (`crates/typer/src/pickle_supply.rs`) was **dropping members of this shape
entirely**, so `symbolOf` gave `not found: value symbolOf`.

The reason for dropping them is to avoid "the implicit cannot be resolved while the type parameter is
undetermined, and the typer silently eta-expands". But the *materialiser* shape — where the section
is implicit-only and that implicit demands the very type parameter in question — is, like
`classTag[Short]`, **always called with an explicit type argument**. So for this shape specifically we
now keep the member. Without an explicit type argument, `T` becomes `Nothing` and the diagnostic is
"implicit not found" (it never turns into an incorrect program).

Effects:

- **Inside a macro implementation it really does resolve.** Since `implicit rTag: c.WeakTypeTag[R]`
  is in scope, the implicits of `symbolOf[R]` / `weakTypeOf[R]` are filled from it.
  That is `val rSym = symbolOf[R]` in slick's `ShapedValue.mapToImpl`.
- **Outside, the tag is materialized** (§7.10); without a universe import in scope the diagnostic
  is the honest `no implicit: could not find implicit value of type TypeTags$TypeTag[Int]`.

#### 3. Diagnosing `reify { … }`

`def reify[T](expr: T): Expr[T] = macro …` on `scala.reflect.api.Universe` is a
**compiler-internal macro** just like quasiquotes: there is no implementation in scala-reflect.jar,
and the pickle entry does not even carry an erased descriptor. So we were saying
`value reify is not a member of JavaUniverse` — the same **lie** as
`value q is not a member of StringContext`.

`Check::report_internal_universe_macro` now says, when the receiver is a universe (or, unqualified,
when an `import <universe>._` is in effect):

```
macro expansion is not implemented: cannot expand reify { ... }.
`reify` is a compiler-internal macro with no implementation in scala-reflect.jar,
so scala-rs would have to reify the expression itself, the way it does
quasiquotes; see docs/macros.md §6.2.
```

This is the diagnostic when `reify` cannot be expanded; the expansion itself is §7.15, over the typed
body since §7.26.

### 7.9 Quasiquoting definitions

**`q"class C(...)"` / `q"case class C(...)"` / `q"trait T"` / `q"object O { ... }"` /
`q"def f(...) = ..."`, and modified definitions such as `q"lazy val a = 1"`, are lowered.** Every shape was read off the real scalac 2.13.16 with
`-Ymacro-debug-lite`, and `tests/fixtures/dq_defs.scala` **compares 101 lines against the real scalac
down to `showRaw`** (run under `java -Xverify:all`; an exact match). The implementation is
`crates/typer/src/reify_defs.rs` (a `#[path]` child module of `reify.rs`).

#### Shapes that are lowered

| Shape | Lowered to |
| --- | --- |
| `q"class C"` | `rs.SyntacticClassDef(mods, name, tparams, ctorMods, paramss, earlyDefs, parents, self, body)` |
| `q"trait T"` | `rs.SyntacticTraitDef(mods, name, tparams, earlyDefs, parents, self, body)` |
| `q"object O"` | `rs.SyntacticObjectDef(mods, name, earlyDefs, parents, self, body)` |
| `q"def f = 1"` | `rs.SyntacticDefDef(mods, name, tparams, paramss, tpt, rhs)` |
| `q"lazy val a = 1"` | `rs.SyntacticValDef(u.Modifiers(rs.FlagsRepr(2147483648L)), …)` |
| `q"var x = 1"` | `rs.SyntacticVarDef(…)` (keeps `MUTABLE`) |
| A trailing implicit clause | `rs.ImplicitParams(<the remaining clauses>, <the implicit clause>)` |
| Type parameters | `u.TypeDef(u.Modifiers(PARAM \| variance), u.TypeName("T"), Nil, u.TypeBoundsTree(lo, hi))` |
| `q"new C(1) { ..$body }"` | `rs.SyntacticNew(Nil, List(<C(1)>), u.noSelfType, <body>)` |
| `q"super.foo"` | `rs.SyntacticSelectTerm(u.Super(u.This(u.TypeName("")), u.TypeName("")), …)` |
| `q"def f: Unit = {..$xs}"` | The right-hand side is `rs.SyntacticBlock(<xs>)` |
| Holes | Names (`q"class $tname"`), parameter lists (`..$params`), type parameters, parents (`extends ..$parents`), and bodies (`{ ..$body }`) |

#### The crux is the flag conversion for `Modifiers`

What `Modifiers` carries are **the bits of `scala.reflect.internal.Flags`**, whose **numbering differs**
from the `Flags` of the scala-rs parser (`PRIVATE` is bit 0 in the parser and bit 2 in nsc). Every
value was read back out of the `FlagsRepr(<n>L)` that `-Ymacro-debug-lite` prints:

| Modifier | nsc bit | Shape used to confirm it |
| --- | --- | --- |
| `PROTECTED` / `OVERRIDE` / `PRIVATE` | `1<<0` / `1<<1` / `1<<2` | `protected def f = 1`, and so on |
| `ABSTRACT` / `DEFERRED` / `FINAL` | `1<<3` / `1<<4` / `1<<5` | `abstract class C` / `val a: Int` / `final class C` |
| `INTERFACE` / `IMPLICIT` / `SEALED` | `1<<7` / `1<<9` / `1<<10` | `trait T` / `implicit val` / `sealed class C` |
| `CASE` / `MUTABLE` / `PARAM` | `1<<11` / `1<<12` / `1<<13` | `case class C` / `var x = 1` / `def f(x: Int)` |
| `COVARIANT` / `CONTRAVARIANT` | `1<<16` / `1<<17` | `class C[+T]` |
| `LOCAL` | `1<<19` | `private[this] val x = 1` |
| `CASEACCESSOR` | `1<<24` | the `x` of `case class C(x: Int)` |
| `TRAIT` = `DEFAULTPARAM` | `1<<25` | `trait T` / `def f(x: Int = 1)` |
| `PARAMACCESSOR` | `1<<29` | class parameters |
| `LAZY` | `1<<31` | `lazy val a = 1` |

Parameter flags **differ between a class and a `def`**. Parameters of a `def` get only `PARAM`, while
class parameters get `PARAMACCESSOR` plus:

- the **first clause** of a `case` class gets `CASEACCESSOR` (later clauses are treated normally),
- non-`case` parameters with no `val` / `var` get `PRIVATE | LOCAL` (they are not members),
- `var` gets `MUTABLE` and a `SyntacticVarDef`.

We also reproduce **the parents nsc's parser fills in**: if no parent is written,
`rs.ScalaDot(u.TypeName("AnyRef"))`; for `case`, `rs.ScalaDot(Product)` and
`rs.ScalaDot(Serializable)` after the written parents (with `case`, `AnyRef` is not filled in).

#### Recovering, again from the original source string, distinctions the parser collapses

- **`class C` versus `class C {}`.** Even when the body is empty, if braces were written nsc's body
  is `List(u.EmptyTree)`, and if they were not it is `List()`. The parser gives `body: []` in both
  cases, so we decide by whether the text of the definition's span ends with `}`.
- **`def f = {..$xs}` versus `def f = $x`.** The parser collapses `{ e }` into `e`, so we decide
  whether to wrap in a `SyntacticBlock` by whether the text immediately before the right-hand side
  ends with `{`.
- **Procedure syntax `def f() { … }`.** nsc fills the result type in with `_root_.scala.Unit`, while
  the parser leaves the type empty. We tell them apart by whether there is an `=` before the
  right-hand side, and reject the form when there is not.

#### Shapes we cannot lower are diagnosed by name (`tests/fixtures/dq_defs_bad.scala`)

| Shape | Diagnostic | Reason |
| --- | --- | --- |
| `q"class C { self => … }"` | a self type … | Indistinguishable from the `List(EmptyTree)` of an empty body |
| `q"class C extends { val x = 1 } with D"` | an early definition … | nsc's `PRESUPER` is bit 37, which does not exist in the parser's (32-bit) flag word |
| `q"private[foo] val x = 1"` | a qualified access modifier (`private[X]`) … | The name field of `Modifiers`. We only carry flags |
| `q"def f(x: => Int) = x"` | a by-name parameter … | nsc's type is `_root_.scala.<byname>[T]`; the parser uses a flag |
| `q"def f(x: Int*) = x"` | a repeated parameter (`T*`) … | As above (`<repeated>`) |
| `q"def f() { 1 }"` | procedure syntax … | As above |
| `q"def f()"` | a `def` with neither a result type nor a body … | nsc fills in `_root_.scala.Unit` |
| `q"{ val (a, b) = e; a }"` | a pattern definition … | The parser desugars it into three definitions; nsc has a single `SyntacticPatDef` |
| `q"class C[F[_]]"` | a higher-kinded type parameter … | Nested type parameters |
| `q"def f[T: Ordering] = 1"` | a context bound (`T : C`) … | nsc desugars this in the typer, not the parser |
| `q"case class C(x: Int) extends ..$parents"` | a `case` class whose parents are a `..$` splice … | Requires concatenating `Product with Serializable` |
| `q"def f(implicit x: Int)(y: Int) = y"` | an implicit parameter clause that is not the last … | `ImplicitParams` covers only a single trailing clause |
| `q"def f = macro Impl.f"` | a `macro` definition … | The right-hand side is not an expression |

#### General holes fixed along the way

| What was fixed | Where |
| --- | --- |
| **`{ case class X(…); … }` was misread as a partial function.** A leading `case` in a block is a **modifier**, not the start of a clause, when what follows is `class` / `object`. A block containing a local `case class` was giving `expected pattern, found class` | `Parser::parse_block_expr` |

### 7.9a The three shapes that need fresh names

**`_` placeholder function literals, `_` type arguments (existentials), and right-associative
operators are lowered.** These three differ from every earlier shape in one
decisive way: **nsc's expansion is a "block", not a single expression**.

```scala
// -Ymacro-debug-lite output for q"_.get" (abbreviating the universe as u
// and u.internal.reificationSupport as rs)
{
  val nn$macro$1: u.TermName = rs.freshTermName("x$");
  rs.SyntacticFunction(
    List(rs.SyntacticValDef(u.Modifiers(rs.FlagsRepr(2105344L)), nn$macro$1,
                            rs.SyntacticEmptyTypeTree(), u.EmptyTree)),
    rs.SyntacticSelectTerm(rs.SyntacticTermIdent(nn$macro$1, false),
                           u.TermName("get")))
}
```

The names are **drawn from the universe's counter at run time** (`freshTermName` / `freshTypeName`).
So scala-rs likewise cannot "hard-code a name": it has to **build the whole block that makes the same
calls**. The implementation gives `Reifier` a `Fresh` state (`crates/typer/src/reify.rs`) that
accumulates the bindings requested while the tree is being built, and `reify` wraps everything in a
block at the end. All three shapes are hoisted into **the same single block** (as in nsc).

#### Shapes that are lowered

| Shape | Lowered to |
| --- | --- |
| `q"_.get"` | `{ val n = rs.freshTermName("x$"); rs.SyntacticFunction(List(rs.SyntacticValDef(mods(PARAM\|SYNTHETIC), n, …)), <the `_` in the body becomes `SyntacticTermIdent(n, false)`>) }` |
| `q"_.foo(_)"` | The same. One fresh name per placeholder |
| `q"(_: Int).get"` | Both the parameter's type slot and the body's ascription are kept, as in nsc |
| `tq"P[_, _]"` | `{ val a = rs.freshTypeName("_$"); val b = …; rs.SyntacticExistentialType(rs.SyntacticAppliedType(<P>, List(rs.SyntacticTypeIdent(a), rs.SyntacticTypeIdent(b))), List(u.TypeDef(mods(DEFERRED\|SYNTHETIC), a, Nil, u.TypeBoundsTree(…)), …)) }` |
| `tq"P[_ <: Int]"` | Upper and lower bounds go into the `TypeBoundsTree` |
| `tq"Option[P[_]]"` | The existential wraps **the application that directly holds the `_` argument** (the same nesting position as nsc) |
| `q"a :: b"` | `{ val n = rs.freshTermName("rassoc$"); rs.SyntacticBlock(List(rs.SyntacticValDef(mods(FINAL\|SYNTHETIC\|ARTIFACT), n, …, <a>), rs.SyntacticApplied(rs.SyntacticSelectTerm(<b>, u.TermName("$colon$colon")), List(List(rs.SyntacticTermIdent(n, false)))))) }` |
| `q"a :: b :: c"` | The blocks nest (two fresh names) |
| `q"b.::(a)"` | **No block.** A dotted call is an ordinary selection |
| `pq"_: R[_, _]"` | A type variable pattern. `u.Bind(u.TypeName("_"), u.EmptyTree)`. No fresh name needed |
| `pq"_: R[_ <: Int]"` | With bounds it is an existential, even inside a pattern |

Every flag value was read back out of the `FlagsRepr(<n>L)` of `-Ymacro-debug-lite`:
`PARAM|SYNTHETIC` = 2105344, `DEFERRED|SYNTHETIC` = 2097168,
`FINAL|SYNTHETIC|ARTIFACT` = 70368746274848 (`ARTIFACT` is `1L << 46`).

#### Recovering, again from the original source string, distinctions the parser collapses

- **`a :: b` versus `b.::(a)`.** The parser makes the right-hand side the receiver of a
  right-associative operator, so both become `Apply(Select(b, "::"), [a])`. nsc builds **different
  trees** for the two (a block for the former, a plain application for the latter). We tell them
  apart by whether the text of the selection node's span **starts with the operator**: infix means
  the span starts at the operator, a dotted call means it starts at the selectee.
- **Placeholder parameters.** The `x$n` the parser creates carries `PARAM | SYNTHETIC`, whereas a
  parameter written in the source carries only `PARAM`. That difference decides whether we "invent a
  name" or "draw a fresh name".
- **`_` type arguments inside patterns.** A bare `_` is a type variable pattern (`Bind`); with bounds
  it is an existential. Whether we are walking under a `pq` / `case` is carried around in
  `Fresh::pat_depth`.

#### Shapes we cannot lower are diagnosed by name (`tests/fixtures/fn2_fresh_bad.scala`)

| Shape | Diagnostic | Reason |
| --- | --- | --- |
| `q"_"` | unbound placeholder parameter | There is nothing to bind. The real scalac rejects it too |
| `tq"_"` | a `_` type argument (an existential) … | As above (nsc says "unbound wildcard type") |

#### Validation: how fresh names are matched up

`tests/fixtures/fn2_fresh.scala` is dual-run against the real scalac 2.13.16 and 32 lines are
compared with `showRaw` (`java -Xverify:all`). The **numbers** in the fresh names do not match as is,
for two reasons, neither of which is a difference in the tree:

1. The counter is global per universe and is shared with every line before this one.
2. nsc hands out names right to left (`q"_.foo(_)"` numbers the argument-side parameter first).

So `renumber_fresh_names` in `crates/cli/tests/quasi.rs` **renumbers from 1 in order of first
appearance, line by line**, before comparing. That drops only the two properties above; **which
occurrence refers to which binder** is not dropped (`_$1 … _$2` and `_$1 … _$1` remain different
strings). The normalization itself is pinned by `renumber_fresh_names_keeps_binder_identity`.

### 7.10 `TypeTag` / `WeakTypeTag` materialization

**`typeOf[T]` / `weakTypeOf[T]` / `typeTag[T]` materialize their tag when no implicit is in scope.**
`c.typeOf[HList]` (in slick's `ShapedValue.mapToImpl`) and `typeOf[Tag]` in `TableQuery` need this.

#### What nsc does (confirmed on the real thing with `-Xprint:typer`)

When the implicit for `def typeOf[T](implicit ttag: TypeTag[T]): Type` is not found, nsc does not say
"not found". It expands the **compiler-internal macro `materializeTypeTag[T](u)`** and **builds** the
tag on the spot:

```scala
scala.reflect.runtime.`package`.universe.typeOf[String](({
  val $u: reflect.runtime.universe.type = scala.reflect.runtime.`package`.universe;
  val $m: $u.Mirror = $u.runtimeMirror(this.getClass().getClassLoader());
  $u.TypeTag.apply[String]($m, {
    final class $typecreator1 extends TypeCreator {
      def apply[U <: scala.reflect.api.Universe with Singleton](
          $m$untyped: scala.reflect.api.Mirror[U]): U#Type = {
        val $u: U = $m$untyped.universe;
        val $m: $u.Mirror = $m$untyped.asInstanceOf[$u.Mirror];
        $u.internal.reificationSupport.TypeRef(…)   // for String this is as far as it goes
      }
    };
    new $typecreator1()
  })
}: reflect.runtime.universe.TypeTag[String]))
```

Inside a macro implementation (`c.typeOf[Hl]`), `$u` is `c.universe` and `$m` is
`c.universe.rootMirror`, and a top-level class takes only the one line
`$m.staticClass("Hl").asType.toTypeConstructor`.
Primitive types such as `Int` do not even get a `TypeCreator`; they use `$u.TypeTag.Int`.

#### The tree scala-rs builds

The implementation is `crates/typer/src/materialize.rs`, entered through `Check::materialize_tag`
(a fallback alongside `classtag_apply_fallback` in `fill_implicit_params_in` — the same position at
which nsc materializes a `ClassTag`).

```text
{
  final class $typecreator1 extends scala.reflect.api.TypeCreator {
    def apply[U <: scala.reflect.api.Universe with Singleton](
        $m$untyped: scala.reflect.api.Mirror[U]): <Types.TypeApi> =
      $m$untyped.staticClass("Foo").asType.toTypeConstructor
  }
  <universe>.TypeTag.apply[Foo](
    <universe>.rootMirror.asInstanceOf[<api.Mirror>], new $typecreator1())
}
```

This is **an ordinary untyped scala-rs tree**, run through `type_expr` just like quasiquote
reification. A local class can stand inside the block because the typer's `TreeKind::Block` is built
to "run the namer on the spot for a `ClassDef` that has no symbol yet", so we can grow one definition
in the middle of implicit search.

Which universe to use is decided by `universe_in_scope()` — the prefix of `import <universe>._` —
the same reading by which a quasiquote decides the universe of a `q"..."`. Without that import we do
not materialize and still say "no implicit", as before.

#### Three points where we differ from nsc (**we do not require the trees to match**)

Rather than the tag tree itself, what we validate is that the **runtime result of `tag.tpe`**
(`toString` / `=:=` / `<:<` / `typeSymbol.fullName`) matches the real scalac 2.13.16
(`tests/fixtures/tt_tags.scala`, 30 lines). There are three differences:

| | nsc | scala-rs | Why |
| --- | --- | --- | --- |
| Binding `$u` / `$m` | binds them to `val`s first | selects `apply`'s arguments directly | The tree is smaller. `tag.tpe` is the same |
| The runtime universe's mirror | `runtimeMirror(getClass.getClassLoader)` | `rootMirror` | `JavaUniverse#runtimeMirror` cannot be supplied yet (its `java.lang.ClassLoader` parameter has no symbol, and `ensure_class` refuses pickle-less classes outside `scala.`). Behavior differs only for classes invisible from the root mirror's class loader, and in that case you get a `ScalaReflectionException` (it never silently produces a different type) |
| The creator's result type | writes `U#Type`, which nsc's erasure turns into `Types$TypeApi` | writes `Types$TypeApi` directly | scala-rs erases abstract type members to `Object` (`erasure::erase_ty`). `TypeCreator.apply` is **abstract**, so a descriptor returning `Object` overrides nothing and the first `tag.tpe` gives an `AbstractMethodError` |

Inserting an `asInstanceOf` on the mirror argument compensates for the same kind of thing.
The type of `rootMirror` is the universe's abstract member `Mirror`, and its upper bound can only be
followed as far as `JavaMirror` in the pickle (the parent of
`JavaMirror extends api.Mirror[self.type]` is dropped by `conv_upper_bound` because the singleton
argument cannot be converted). The value really is a `Mirror`, so the cast becomes a `checkcast` that
always succeeds.

#### Holes plugged on the supply side

Several things were missing before `u.TypeTag.apply` could be called.

| What was fixed | Where |
| --- | --- |
| **`TypeTags$TypeTag$` had no symbol.** The classfile of an object nested in a trait has no `ScalaSignature` of its own (the pickle is inside the enclosing `TypeTags`), so `install_classpath` skips it. As a result the descriptor `()Lscala/reflect/api/TypeTags$TypeTag$;` stayed an unresolvable `Type::Named` and we got `value apply is not a member of TypeTags$TypeTag$`. We now build the `ModuleClass` and insert `apply[T](Mirror, TypeCreator): TypeTag[T]` **by hand**. The erased descriptor is written out literally (if a method symbol's `jvm_name` starts with `(` it is taken as the descriptor — the same convention as pickle supply). The pickle's signature is `Mirror[TypeTags.this.type]`, and scala-rs cannot spell that singleton argument | `materialize::ensure_tag_module` |
| **The implicit parameter of `TypeTags#typeOf` was a `Type::Named`.** The pickle subset `install_classpath` reads holds member types by **simple name**, so nobody had installed the name `TypeTags$TypeTag` and it was unresolved. That was the true identity of `no implicit: could not find implicit value of type TypeTags$TypeTag[Foo]`, and erasure was about to write a descriptor out of that type | `materialize::resolve_named_tags` |
| **Sometimes the `TypeTags#TypeTag` accessor itself is absent.** If `TypeTags` is read as a classfile then `TypeTag()` appears in the method list, but when it comes via the pickle (nobody named it during the classpath scan) the module member is not among what `complete_named` installs, and the accessor is missing entirely. We write the descriptor and declare it here. Furthermore `TypeTags` is not a **direct** parent of `JavaUniverse` (it is a parent of `api.Universe`, and that link exists only in the pickle), so we first let `supply_from_pickle` walk the ancestors — otherwise **only the first `typeOf[T]` of a run** failed with "value TypeTag is not a member of JavaUniverse" | `materialize::ensure_tag_module` / `Check::materialize_tag` |
| **A resolved type cannot be spliced in as a type tree.** Neither the `T` of `TypeTag.apply[T]` nor the `api.Mirror` we cast to has a path reachable by name at the use site (`scala.reflect.api.Mirror` is not imported). We place the marker `Ident("$resolvedType")`, the counterpart of nsc's `TypeTree(tp)`, and `tree_to_type` returns its `ty` unchanged | `materialize::RESOLVED_TYPE` / `Check::tree_to_type` |

#### Shapes it builds, and shapes it refuses

`staticClass(<name>)` is a call that names **one class**. This section first built only class
types with no type arguments; the creator's body now composes several shapes:

- a class: `staticClass("N").asType.toTypeConstructor`;
- an applied type constructor, including tuples, function types and arrays:
  `appliedType(staticClass("N"), List(<each argument>))` (§7.12, §7.13);
- a type parameter with a tag in scope: that tag, rebased onto the creator's mirror (§7.12);
- nested classes, singletons, aliases and type parameters with no tag (as a free type, for a
  `WeakTypeTag`), through the type reifier `reify` uses (§7.26).

Refused by name (pinned by `tests/fixtures/tt_tags_bad.scala`): a `TypeTag` for a type parameter with
no tag in scope (nsc refuses it too: `No TypeTag available for T`), and a refinement type, which needs
nsc's `newNestedSymbol` scope reification.

The point is **never to silently build a different type**. A wrong tag is not a compile error; it just
arrives at the macro at run time as a "different `Type`", which makes it the hardest kind of defect to
find after the fact.

#### Validation

- `tests/fixtures/tt_tags.scala` — compiled and run with **both** scala-rs and the real scalac
  2.13.16, with the 30 lines of output matching exactly (`java -Xverify:all`).
  `tt_tags_materialises_type_tags` / `tt_tags_matches_real_scalac` in `crates/cli/tests/quasi.rs`.
- `tests/fixtures/tt_ctx.scala` — `c.typeOf[HL]` / `c.weakTypeOf[Rep]` inside a macro implementation
  (the shape of slick's `mapToImpl`). Both compilers accept it and the classfile loads and verifies
  on the JVM.
- `tests/fixtures/tt_tags_bad.scala` — the refused shapes are diagnosed by name.

### 7.11 The engine: calling macro implementations

The §2.3 prototype became production code: **a call to `def f = macro Impl.m` is really expanded,
and the expanded program runs**. It is dual-run against the real scalac
2.13.16 in the same two-file, two-compilation configuration, and **the program output matches
exactly** (`crates/cli/tests/engine.rs`).

#### The shape (how the bridge is put together)

The engine is **a single Java file** (`crates/typer/java/ScalaRsMacroEngine.java`) that touches Scala
classes **entirely through reflection**. So `javac` does not need scala-reflect.jar, and no classfile
is checked into the repository. It is embedded in the binary with `include_str!` and, on the first
expansion, written out to

```
$TMPDIR/scala-rs-macro-engine-<FNV hash of the source>/
```

and compiled with `javac` (the hash means a stale classfile can never run).

- **One resident process per compilation.** The first expansion starts `java`, and everything after
  that goes over a pipe with one request per line, so the JVM's startup cost is paid once. It is
  killed from `Drop` when the `Typer` goes away.
- **The classpath is `binary_path` itself** (`-cp` plus `--scala-library`). This mirrors nsc, whose
  `-Ymacro-classpath` defaults to the compilation classpath, and it also satisfies the caveat found
  in §2.3 that "reify's `staticModule` also demands the classes being compiled".
- **The `Context` is a `java.lang.reflect.Proxy`** (as in the prototype). The members it answers
  are listed in §3.1, plus the traits' default implementations (`invokeDefault`). Everything else
  fails with `UnsupportedOperationException`, and the Rust side **puts that name in the diagnostic**.
- **Serialization is S-expressions** (§4.2). Both ends parse them with a few dozen lines of code,
  and they ride the pipe one message per line.

```
→ (expand "EgImpl$" "plusImpl" (argss (args (arg expr <tree> (ty "scala.Int")))) (tags))
← (ok (t "Apply" (s0) (t "Select" (s0) (t "Literal" (s0) (c "Int" "41")) (n term "$plus"))
        (l (t "Literal" (s0) (c "Int" "1")))))
```

**The returned tree is written generically by the engine.** The engine does not know the node kinds:
it lays out `productPrefix` and `productElement` as they come, and attaches a fully qualified name to
a `Symbol` only when `isStatic`. Deciding "this shape cannot be built" happens **only on the Rust
side**, and an unknown `Prefix` always becomes a diagnostic that names it.

#### Where expansion happens

As in nsc, expansion happens **inside the typer**, at **the outermost node of the macro application**
(at the end of `Check::type_expr`, **before** `adapt`). "Outermost" is detected with a single bit,
`typing_callee`: it is set just before `Apply` / `TypeApply` types its callee and is `mem::take`n at
the entry of `type_expr`. So `M.f` is not expanded as the head of `M.f(1)`, while the `M.g(1).h`
inside a receiver is. The inner `Apply` of a curried macro is rejected as "still a `Type::Method`".

For a blackbox macro the expansion result is typechecked exactly once against **the declared return
type** as the expected type, and the type is put back to that declared type (nsc's
`Typed(expanded, TypeTree(innerPt))`). A whitebox macro keeps the expansion's own type (§7.30).

**Everything that could not be expanded becomes a diagnostic, without exception.** The
`report_macro_calls` sweep reports every macro call left in the tree; the expander merely records the
**reason** for each failure per span and hangs it there:

```
error: macro expansion is not implemented: cannot expand nameOf
       (implementation EgImpl$.nameOfImpl): scala-rs cannot build a type tag for
       `Main.type`, a singleton type. See docs/macros.md.
```

(`List[Int]` was this example's refusal until §7.21 made a tag descriptor carry
its type arguments; a singleton type is one of the shapes still refused.)

#### Two-pass compilation is by design

nsc decrees that "a macro implementation must have been compiled **before the run in which the
expansion happens**" (§1.3). scala-rs is the same: if the implementation is not on the macro
classpath, the engine returns `ClassNotFoundException`, which becomes the reason
`is not on the macro classpath (nsc requires the implementation to have been compiled by an earlier
run)` (pinned by `tests/fixtures/eg_samerun_bad.scala`).
The macro **def** side may live in the current run (that is slick's shape too).

#### The first shapes that worked

Later sections widened each row (§7.12, §7.13, §7.21–§7.25).

| Shape | Example | Notes |
| --- | --- | --- |
| No arguments | `def const(): Int = macro EgImpl.constImpl` | The expansion is `Literal(Constant(42))` |
| A `c.Expr[T]` argument | `def plus1(x: Int): Int` | The call site's tree is wrapped in an `Expr` and passed |
| A raw `c.Tree` argument | `def twice(x: Int): Int` | The 2.11-and-later shape. This is what slick's `mapToImpl` uses |
| `c.WeakTypeTag[T]` | `def nameOf[T]: String = macro EgImpl.nameOfImpl[T]` | |
| Expansion result trees | `Literal` / `Ident` / `Select` / `Apply` / `TypeApply` / `Block` / `If` / `Typed` / `This` / `EmptyTree` / `TypeTree` | Anything the rebuilder does not know is refused by name |
| Static symbols | The `Ident(EgHelper)` of an expansion | If `isStatic`, expand to the fully qualified path and resolve at the call site |

#### Two general holes plugged on the way

| What was fixed | Where |
| --- | --- |
| **`blackbox.Context` was not standing as an interface.** The placeholder in `prelude_reflect` had `Flags::EMPTY`, and **that symbol is used as the real thing** even in runs where scala-reflect.jar is present (`ensure_class` returns it via `find_by_jvm`). As a result the `c.universe` of a macro implementation became an `invokevirtual` and gave **an `IncompatibleClassChangeError` the moment it ran**. The §7.6 fixtures only checked "the classfile loads and verifies", so this went unnoticed | `prelude_reflect::ctx` |
| **The placeholder stayed a class even though the pickle said trait.** A symbol built by `find_or_stub_java_class` from a descriptor does not know trait from class. `give_stub_its_kinds` only fixed up classes **with** type parameters, so a trait with **no** type parameters, such as `scala.reflect.macros.Universe`, stayed a class | `PickleSupply::give_stub_its_kinds` |

#### Validation

- `tests/fixtures/eg_impl.scala` + `tests/fixtures/eg_use.scala` — compiled in two stages with
  scala-rs and run, with the 8 lines of output matching `tests/fixtures/expected/eg_use.txt`
  (`java -Xverify:all`). **A separate test pins that the same two files, compiled in two stages by the
  real scalac 2.13.16 and run, produce the same 8 lines.** A macro that expands into "a different
  tree" still compiles, so **only comparing the output can catch a wrong expansion**.
- `tests/fixtures/eg_samerun_bad.scala` — the case where the implementation is in the same run.
- `tests/fixtures/eg_gaps_bad.scala` — argument shapes that cannot be passed, and tags that cannot be
  built.

### 7.12 `c.Expr[T](tree)` and `c.prefix`

`c.Expr[T](tree)` resolves to the `Context` factory, `c.prefix` carries the call site's receiver, and
**we assemble the `WeakTypeTag[F[E]]` that `c.Expr[F[E]]` demands**. With all three in place,
**a macro of the same shape as slick's `TableQueryMacroImpl.apply`** can be written and expanded, and
its program output matches the real scalac 2.13.16 in a dual run (`tests/fixtures/ex_impl.scala` +
`tests/fixtures/ex_use.scala`).

#### 1. `c.Expr[T](tree)` — value-position collapsing happened too early

`scala.reflect.macros.Aliases` declares `Expr` **twice**:

```scala
val Expr: universe.Expr.type                       // the extractor object
def Expr[T: WeakTypeTag](tree: Tree): Expr[T]      // the factory method
```

The selection `c.Expr` starts out as a `Type::Overload`, but `maybe_auto_apply` applied
**SLS 6.26.3 (in value position, keep only candidates that take no parameters)** on the spot and
collapsed it to the `val`. The collapsed result is the module `universe.Expr$`, so the following
`[Int]` rode the module → `apply` redirect, hit `universe.Expr.apply(Mirror, TreeCreator)` and gave
`no matching overload`.

nsc's ordering is the opposite: **explicit type arguments narrow the overloads first**. So:

- When a selection collapses, we now also record the set **on the surviving symbol**
  (`overload_member_types` / `overload_groups`, since the key the caller holds is the post-collapse
  symbol, not `found[0]`).
- `TypeApply` swaps in another candidate only when **exactly one** candidate matches the number of
  type arguments and the symbol currently held has a different type parameter count
  (`Check::alt_taking_targs`). Since this only happens when the set genuinely had two or more members,
  the existing "one candidate" path, as in `Ordering[String]`, passes straight through.

#### 2. `c.prefix` — the receiver at the call site

If what `peel_application` finds after stripping `Apply` / `TypeApply` is a `Select`, its `qual` is
the prefix. We send **only the tree** to the engine, and the engine builds
`Expr[Nothing](prefixTree)(TypeTag.Nothing)` as nsc does (since blackbox's `PrefixType` is an
abstract member, `c.prefix.staticType` is `Nothing` in nsc too; a fixture pins this).

A receiver we cannot carry (`new`, a block, a call with no receiver) **is not an error on the spot**.
Whether the implementation reads `prefix` is unknowable from the call side, so **we send the reason
string along** and the engine throws with that reason only if `prefix` is actually read.
An implementation that does not read it expands straight through.

#### 3. Assembling `WeakTypeTag[F[E]]`

`c.Expr[ExBox[E]](tree)` demands an implicit `WeakTypeTag[ExBox[E]]`. The materialiser of §7.10
handled only **monomorphic classes** buildable from a single `staticClass`, so this got stuck.
We generalized the creator's body into a synthesis of three shapes (`materialize::TagBody`):

| Shape | Tree generated |
| --- | --- |
| A monomorphic class | `$m$untyped.staticClass("N").asType.toTypeConstructor` (as before) |
| An applied type constructor | `$m$untyped.universe.appliedType($m$untyped.staticClass("N"), List(<each argument>))` |
| A type parameter | `<the tag in scope>.in($m$untyped).tpe` |

`appliedType(sym, args)` is the public version of what nsc writes as
`internal.reificationSupport.TypeRef(thisPrefix(owner), sym, List(…))` (a symbol's `typeConstructor`
is `TypeRef(owner.thisType, sym, Nil)`, so it comes out as the same `TypeRef`). Tags for type
parameters are looked up by **ordinary implicit search**. Materialisation is the fallback *after*
search has failed, so there is no cycle.

Shapes we cannot build are refused by name. Because the synthesis **recurses**, a constructor one of
whose arguments cannot be built names that argument. (Tuples, function types and arrays are built
since §7.13; nested classes and free types since §7.26.)

**One known divergence**: for a constructor reached through a **type alias** such as `Predef.Map`,
nsc's creator preserves the alias (`selectType(staticModule("scala.Predef"), "Map")`), whereas
scala-rs does a `staticClass` on the class the alias points at. The two are `=:=` and have the same
`typeSymbol`, but `toString` differs: `Map[String,Foo]` versus
`scala.collection.immutable.Map[String,Foo]`.
It is the same divergence §7.10 already recorded for `Predef.String`; with `String` the rendering just
happened to coincide. For `Map`, `tt_tags.scala` compares `=:=` and `typeSymbol.fullName` (not
`toString`).

#### 4. `New` in expansion results

In reflect, `new C(args)` is `Apply(Select(New(tpt), termNames.CONSTRUCTOR), args)`; in the scala-rs
tree it is `Apply(New(tpt), args)`. We now accept `New` and fold away the `<init>` selection on top of
it. Slick's `TableQueryMacroImpl` writes `New(TypeTree(e.tpe))`, so this is needed.

#### Validation

- `tests/fixtures/ex_impl.scala` + `tests/fixtures/ex_use.scala` — compiled in two stages with
  scala-rs and run, matching `tests/fixtures/expected/ex_use.txt` (`java -Xverify:all`).
  **A separate test pins that the same two files, compiled in two stages by the real scalac 2.13.16
  and run, produce the same 10 lines.** The output includes
  `weakTypeOf[ExBox[E]].toString` (i.e. the type of the synthesized tag) and
  `c.prefix.staticType.toString`, so **if we built the tag or the prefix differently from nsc, the
  lines would change**.
- `tests/fixtures/tt_tags.scala` — materialisation outside a macro, including
  `List[Int]` / `Option[Foo]` / `List[List[Int]]`; even the string of `tag.tpe` matches the real
  scalac.
- `tests/fixtures/ex_notag.scala` — tags this section refused, built since §7.26 (formerly
  `ex_notag_bad.scala`).
- `tests/fixtures/ex_gaps_bad.scala` — the two kinds of receiver we cannot carry.
  The real scalac accepts both, so these fixtures pin holes on the scala-rs side.

### 7.13 `Function` / `ValDef` in expansion results

**An expansion result may contain `Function` and `ValDef`**, so the tree slick's
`TableQueryMacroImpl.apply` assembles —
```scala
Function(
  List(ValDef(Modifiers(Flag.PARAM), TermName("tag"),
              Ident(typeOf[Tag].typeSymbol), EmptyTree)),
  Apply(Select(New(TypeTree(e.tpe)), termNames.CONSTRUCTOR),
        List(Ident(TermName("tag")))))
```

— makes the full round trip, and the expanded program runs. It is dual-run in two-stage compilation
against the real scalac 2.13.16 and the output matches exactly (`tests/fixtures/sd_impl.scala` +
`tests/fixtures/sd_use.scala`).

#### 1. `Modifiers` is carried **by name**

Building a `ValDef` requires `Modifiers`. Since the engine forwards `productElement` as is,
`Modifiers` used to come across as the `toString` `(o "Modifiers(PARAM)")`.

We do not send the number (a `FlagSet` is a `Long`). nsc's bit layout is an internal detail, and
moreover **one bit carries two names** (`BYNAMEPARAM` is `COVARIANT`, `DEFAULTPARAM` is `TRAIT`).
So the engine **reflectively enumerates the zero-argument, `long`-returning methods of
`universe.Flag`** and writes out the name of every bit that is set. Leftover bits with no name are
appended in hexadecimal.

```
(mods (f "PARAM") (rest "0") "" (l))
```

The Rust side maps the names onto its own `Flags`. **Both a name that is not in the table and unnamed
leftover bits are diagnostics** (`the expansion contains a definition marked `DEFERRED`, a modifier
scala-rs cannot rebuild yet`). Dropping them silently would turn a `var` into a `val` and a
`lazy val` into a strict `val`, and nobody would notice.
For bits with two names we take **the reading appropriate to a `ValDef`**, the only kind of definition
this expander builds (`BYNAMEPARAM` / `DEFAULTPARAM`). `privateWithin` and annotations are carried
too (annotated ones are currently a diagnostic).

#### 2. Three general holes plugged along the way

| What was fixed | Where | Impact |
| --- | --- | --- |
| **`import c.universe._` was losing to the implicit `import scala._`.** `expose_unqualified` searched in the order "enclosing package → `scala._` → `java.lang._` → root → **wildcard imports**". Under SLS 2 an explicit import ranks higher (`scala._` / `java.lang._` are the outermost wildcard imports). So `Function(vparams, body)` resolved to `scala.Function` (an object with no `apply`), and **the macro implementation slick actually writes could not be compiled at all** | `Check::expose_from_wildcards` | The wildcard stage was moved ahead of `scala._`. Names installed eagerly are already in the current scope and never take this path, so the effect is limited to "names read lazily from the pickle" |
| **Writing `scala.Int` did not give a primitive.** Written as a path, `scala.Int` hits package member lookup and becomes a `Type::Class`. It renders as `Int` but is equal to nothing, so `val x: scala.Int = 1` gave `type mismatch; found: 1  required: Int` | `check::scala_value_type` | A `TypeTree(typeOf[Int])` in an expansion result arrives as a fully qualified name, so this path is needed as is |
| **Tags for tuples, function types and arrays could not be built** | `Check::tag_body` | Name `scala.TupleN` / `scala.FunctionN` / `scala.Array` explicitly and put them on the `appliedType` synthesis of §7.12. Slick's `c.Expr[Tag => E]` demands this. `tt_tags.scala` pins that even `toString` matches the real scalac |

#### Validation

- `tests/fixtures/sd_impl.scala` + `tests/fixtures/sd_use.scala` — compiled in two stages with
  scala-rs and run, matching `tests/fixtures/expected/sd_use.txt` (`java -Xverify:all`).
  **A separate test pins that the same two files, compiled in two stages by the real scalac 2.13.16
  and run, produce the same 6 lines.** A `Function` with the parameter names mixed up, or a `ValDef`
  with the modifiers dropped, both still compile, so **only comparing the output can catch them**.
- `tests/fixtures/sd_gaps_bad.scala` — the two shapes we refuse.
- `tests/fixtures/tt_tags.scala` — tags for tuples, function types and arrays added.

#### 3. Applying the result of a macro that takes no arguments

In `SdUse.adder(20, 22)`, when `adder` is a macro that **takes no arguments**, the `Apply` is not the
macro's own argument clause but **an application of the expansion result**. The expander was stripping
`Apply` unconditionally, so it produced the incorrect diagnostic
`the implementation takes 0 argument(s) and the call site supplies 2` — against a call the real scalac
accepts.

We now count the macro def's own parameter clauses (the `paramss` of the symbol's `Type::Method`) and
**descend into** any excess layers, expanding there. The layers are not necessarily plain `Apply`s:
applying a function value goes through an `apply` selection the typer inserts, so rather than counting
layers and descending, we look for the node "whose head is that macro and whose clause count matches
exactly" (`macro_application_node`). The outer `Apply` **still holds** the macro def's symbol, so we
drop it. Leaving it in makes `report_macro_calls` report "an unexpanded macro" — in a form that does
not even have a reason string.

### 7.14 Nested `object`s and `<val>.type`

Two holes stood between scala-rs and the tree `reify { … }` has to build (§7.15): `c.universe.Expr`
(a nested `object` of the universe) could not be reached, and `Mirror[c.universe.type]` could not be
written. Neither is `reify`-specific; both are general features that also help code unrelated to
macros.

#### 1. `object`s inside a trait were not being supplied

`trait Exprs { object Expr { … } }` compiles to an interface method
`Expr()Lscala/reflect/api/Exprs$Expr$;` plus the module's own classfile.
`PickleSupply::complete_named` reads only `Def` and `Val` from the pickle, so
`MemberKind::Module` entries were **discarded entirely**. As a result,

- `c.universe.Expr` → `value Expr is not a member of Universe`
- `Expr` under `import c.universe._` → `not found: value Expr`

both of which are **lies** (the member is in the pickle).

We added `PickleSupply::install_nested_module`. It installs the module class under the JVM name
`Outer$Name$` and erects a zero-argument accessor on **`class_sym` (the receiver class the search
started from)**. We abandoned the idea of putting it on the declaring trait:
`Check::qualify_term_import` matches "the member's owner" against the import prefix's class to rewrite
a bare name under `import u._` back to `u.name`, but the pickle parents of library classes are linked
only one step at a time, so an accessor placed on a trait far away in the linearisation was not
recognized as "belonging to this import" and we emitted `Main$.Expr()`, giving a
`ClassCastException`. The same convention as `install` (install on the receiver class) is the correct
one.

We let `erased_desc` decide the call target. The classfile of `api/JavaUniverse` has `interfaces: 0`,
so `invokevirtual JavaUniverse.Expr()` does not resolve (`NoSuchMethodError`). We record
`declaring_class` / `declaring_is_interface` and name that class with a `checkcast` in between — the
same shape as nsc.

**Broken accessors originating from classfiles are repaired.** When `adopt_binary_class` reads
`Exprs.class` it installs `def Expr(): Exprs$Expr$` from the descriptor, but since nobody has created
a symbol for `Exprs$Expr$` the return type stays an unresolved `Type::Named`, `class_sym_of` returns
`None` and `c.universe.Expr.apply` gave `value apply is not a member of Exprs$Expr$`.
Return types that are already resolved are **left alone** (we add precision but never take members
away).

`materialize::ensure_tag_module` used to treat "there is a module class" as the marker that its job
was done, but since this supply path now creates the module class first, the marker was changed to
**"there is an `apply`"**. Double registration of the accessor was likewise changed to "do not add one
if there is already one pointing at the same module class".

#### 2. `c.universe` could not be written as a stable identifier in a type

`Mirror[c.universe.type]` gave `stable identifier required, but c.universe found`. The cause was not
`member_is_stable` but **`Check::term_path_sym`**, which accepted only
`SymKind::Term | Module | ModuleClass`. A `val` read from a pickle is installed as a zero-argument
**`SymKind::Method`** (a classfile cannot distinguish a `val` accessor from a plain `def`) with
`Flags::ACCESSOR` set, so it was being dropped. The inconsistency is that `c.universe.Tree` goes
through `path_dependent_type` and only calls `member_is_stable` (which does look at `ACCESSOR`), so
it worked.

The three readers of `Type::SingleType { sym }` (`class_sym_of` / `expand_in_type` / `erase_ty`) were
looking at `sym.ty` directly, so they now go through `SymbolTable::singleton_underlying`, which opens
a zero-argument `Method` into its result type.

#### 3. Three general holes plugged along the way (all of them **silently broken** shapes)

| What was fixed | Where | Symptom |
| --- | --- | --- |
| **A method's parameters looked like "members" of that method.** `install` allocates parameter symbols under the method's owner, so when `qual.sym` is a method (i.e. the callee of an application), `lookup_member(qual.sym, name)` picks them up | the `qual.sym` fallback in `Check::type_select` | `m.staticClass(n).fullName` resolved to `staticClass`'s **parameter `fullName`**, and codegen emitted a `Fieldref` with "owner class = the method's erased descriptor". `ClassFormatError: Illegal class name "(Ljava/lang/String;)L…;"` — **the compile succeeds silently** |
| **The `declaring_class` `checkcast` was missing on parenless selections.** The `Apply` path inserts it via `checkcast_erased_method_receiver`, but the standalone `Select` path did not | the `SymKind::Method` branch of `gen::gen_select` | `u.Expr` left `JavaUniverse` on the stack and did `invokevirtual Universe.Expr()`. `VerifyError` |
| **The receiver of a member `object` was being thrown away.** When the qualifier is a zero-argument accessor (its type is a `Type::Method`), `class_sym_of` cannot answer, and if the pickle parents are not linked `is_owner_compatible` is false as well, so we fell through to `load_module_instance` and pushed **the `this` of the enclosing source class** | `gen::gen_module_member_receiver` | `universe.Liftable[String](f)` pushed `aload_0` and gave `ClassCastException: Main$ cannot be cast to scala.reflect.api.Liftables`. **The compile succeeds silently** |

We also made `gen_receiver` strip `TypeApply` / `Typed` (in `o.P.apply[T](x)` the function is wrapped
in a `TypeApply`, and the fallback branch was looking only at `fun.sym`).

#### Validation

- `tests/fixtures/rd_nested.scala` — against the runtime universe, uses nested `object`s
  (`Expr` / `Liftable`) through a path and through a wildcard import, plus
  `Mirror[scala.reflect.runtime.universe.type]`, printing 5 lines.
  **The real scalac 2.13.16 produces the same 5 lines**
  (`tests/fixtures/expected/rd_nested.txt`). A member object with the wrong receiver still compiles,
  so **there is no way to catch it other than running it**.
- `tests/fixtures/rd_impl.scala` + `tests/fixtures/rd_use.scala` — **the `reify`
  shape, written out by hand and actually expanded and run**. See item 4 below.
  `rd_impl` uses `c.universe.Expr` both through a path and through a wildcard import, uses
  `Mirror[c.universe.type]` as a type argument, and builds three `TreeCreator`s.
  Compiled in two stages with scala-rs and run it prints 3 lines, and **the same two files, compiled
  in two stages by the real scalac 2.13.16 and run, give the same 3 lines**
  (`tests/fixtures/expected/rd_use.txt`). A creator that resolved a static symbol in a different
  universe, and one that forgot to rebase a splice, **both compile**, so only comparing the output
  can catch them.

#### 4. Writing `Exprs#Expr.apply` out by hand

The expansion of `reify` ends by calling `c.universe.Expr.apply[T](mirror, creator)`. Even once `Expr`
became reachable, this `apply` **could not be called**: the pickle's signature is

```text
def apply[T](mirror1: Mirror[Universe.this.type], treec: TreeCreator)
            (implicit tag: WeakTypeTag[T]): Expr[T]
```

and `Universe.this.type` is converted against "the class being completed", which is the module `Expr$`
itself, so the first parameter became `Mirror[Expr$]` and matched no call
(`no matching overload for (Mirror[Expr$], TreeCreator)(WeakTypeTag[T])Exprs$Expr[T]`).
This is exactly the same reason `materialize::ensure_tag_module` writes `TypeTag.apply` out by hand,
so we treat it the same way (`PickleSupply::install_expr_apply`, with the erased descriptor written
out too). The implicit clause is kept as is, so a hand-written
`c.universe.Expr.apply[T](m, creator)` receives its `WeakTypeTag[T]` from the materialiser of §7.10.

With this, **the tree `reify` ought to build works end to end when written by hand**: the three macros
in `rd_use.scala` really are expanded by the engine and print `42 / 42 / true`. §7.15 builds it
automatically from `reify { … }`.

#### The upper bound of `u.Mirror`

`Mirrors#Mirror` is `type Mirror >: Null <: api.Mirror[self.type]`, and `conv_upper_bound` drops this
bound (the singleton argument cannot be converted). So the `mm` of `x.in[u.type](mm)` has to be cast
to `scala.reflect.api.Mirror[u.type]` rather than `u.Mirror` before being passed (nsc writes the
former). `rd_impl.scala` does this by hand, and `reify`'s expansion binds its mirror local the same
way (§7.15).

### 7.15 Expanding `reify { … }`

The tree that §7.14 got working "end to end when written by hand" is built by the compiler.
`crates/typer/src/reify_expand.rs` builds nsc's expansion shape (measured with `-Xprint:typer`):

```text
{ final class $treecreator1 extends scala.reflect.api.TreeCreator {
    def apply[U <: scala.reflect.api.Universe with Singleton](
        $m$untyped: scala.reflect.api.Mirror[U]): <Trees.TreeApi> = {
      val $u = $m$untyped.universe
      val $m = $m$untyped.asInstanceOf[scala.reflect.api.Mirror[$u.type]]
      <body>
    }
  }
  <universe>.Expr.apply[T](
    <universe>.rootMirror.asInstanceOf[<api.Mirror>], new $treecreator1()) }
```

The differences from nsc are the same three as in `crate::materialize` (use `rootMirror`, write the
creator's result type as the bound `Trees$TreeApi` rather than `U#Tree`, and insert a cast on the
mirror), for the same reasons. `val $m` is emitted only when the body needs it.

#### The body

The body is lowered by the same `Reifier` (`crates/typer/src/reify.rs`) that lowers quasiquotes,
running in a "reify mode" that resolves references **by symbol rather than by name**: a static
`object` becomes `$u.internal.reificationSupport.mkIdent($m.staticModule("<full name>"))`, a
`x.splice` becomes `x.in[$u.type]($m).tree`, and a type argument becomes `mkTypeTree(<type>)` built
the way a `TypeTag` is (`crate::materialize::TagBody`). Building a reference by its written name
would **compile and run**, pointing at whatever happens to have that name at the expansion site --
precisely the bug reification exists to prevent.

This section's first version classified each identifier of the *parsed* body by typing it on its own
and refused locals, parameters, definitions, closures and most other shapes by name. §7.26 replaced
that with a walk over the **typed** body, which follows nsc's own rules (free terms for locals and
parameters, free types, definitions reified by name, `mkThis`); the design is in
[`docs/notes/reify-design.md`](notes/reify-design.md).

#### We handed the source string to the typer

`Reifier` uses `src` (the original source) to recover distinctions the parser folds away
(`A => B` versus `Function1[A, B]`, `(a, b)` versus `Tuple2(a, b)`, `a :: b` versus `b.::(a)`).
For quasiquotes the body is a string reassembled by `quasiquote.rs`, so it was right there; but the
body of a `reify` is **text from a real file**. `Typer` did not hold the source, so we added
`typecheck_units_src` / `typecheck_opts_src` and pass in the `SourceFile::src` the driver already has.
For calls that do not pass it (unit tests that type a snippet) it is empty, and each read falls to the
written-out branch.

#### Validation

`tests/fixtures/rb_impl.scala` + `rb_use.scala` are compiled in two stages and print 16 lines, and
**the same two files, compiled in two stages by the real scalac 2.13.16 and run, give the same 16
lines** (`tests/fixtures/expected/rb_use.txt`). The last two lines fill a splice with a side-effecting
expression, so if the tree dropped a splice or built one twice the count would change.
`rb_free.scala` (formerly `rb_bad.scala`, the shapes this section refused) runs them since §7.26.

### 7.16 What compiling `ShapedValue.mapToImpl` needed

`slick.lifted.ShapedValue` — of which §3.3 said "the body is almost entirely quasiquotes" — compiles
with no errors. It needed the four fixes below and two smaller ones.

#### 1. `MemberScope` cannot be read as an `Iterable[Symbol]`

`rTag.tpe.decls.collect { … }` — the first line of `mapToImpl` — gave
`value collect is not a member of Scopes.MemberScope`. The real scala-reflect's
hierarchy is

```text
type MemberScope >: Null <: AnyRef with Scope with MemberScopeApi
trait MemberScopeApi extends ScopeApi
trait ScopeApi extends Iterable[Symbol]
```

and neither `MemberScopeApi` nor `ScopeApi` **has a pickle of its own** (the classfile of
`Scopes$MemberScopeApi` has `interfaces: 0`; the parents are written only in the pickle of
`Scopes.scala`).

`PickleSupply::complete` was shaped as "if it is not in the class's own pickle, ask the library
ancestors too", but that ancestor list was **a snapshot of the parent list at the moment
`library_ancestors` was called**. A stub's parent list is empty until the pickle is read, so
**a climb of two or more steps stopped at the first**: we reached `MemberScopeApi`'s pickle parent
`ScopeApi`, and even though `complete_on(ScopeApi)` attached `Iterable[Symbol]` immediately
afterwards, nobody ever asked `Iterable`.

We replaced it with `complete_on_ancestors`, which **calls `ensure_parents` at each step before moving
to the next**. The order (parents from the back, breadth first — the same linearisation as
`Check::enter_inherited_members`) is unchanged.

#### 2. Members read through an abstract type member were not substituted

After 1, `collect` is found, but `decls.toList` returns `List[A]` (still `Iterable`'s own type
parameter). The `walk` of `SymbolTable::subst_as_seen_from` had no branch for
`Type::TypeMember` / `Type::TypeParam` and fell through to `_ => ty`.
**A member read from an abstract type member is declared by that member's upper bound**, so we now
follow the bound and substitute. With that, the element type of `decls` really is `Symbol`, and
`s.isVal` / `s.isCaseAccessor` / `s.typeSignature` resolve.

#### 3. `blackbox.Context { type PrefixType = … }`

Slick writes `c: blackbox.Context { type PrefixType = ShapedValue[?, U] }`.
`macro_context_kind` looked only at `Type::Class`, so a refinement gave
`must take scala.reflect.macros.blackbox.Context … as its first parameter`.
We added two more candidates:

* **The refinement's parent** (when read from source). A refinement only fixes members; what decides
  blackbox versus whitebox is the parent.
* **The erased descriptor of the first parameter** (a last resort). scala-rs's own pickle drops
  refinements, so reading back from our classfile gives `Any`. Descriptors are not refined, so that is
  where the answer is. If the first parameter really is `Any`, the descriptor is `java.lang.Object`,
  which is neither `Context`, and we refuse as before. For an implementation in source (with no
  classfile) the descriptor is unavailable, so the diagnostic is not weakened.

#### 4. Mixing `..$xs` with ordinary elements

`Reifier::splice_clause` only built "all ordinary" or "exactly one `..$xs`" and refused any mixture.
We made it match nsc's `reifyList`:

> Group runs of consecutive ordinary elements into a single `List(...)`, leave rank-1 holes as they
> are, and join them left to right with `++`.

`q"f(a, ..$xs, b)"` → `List(<a>) ++ xs ++ List(<b>)`. The argument order is the concatenation order,
and every fragment is already a `List[Tree]`, so there is nowhere to have to guess a static type.
It applies in four places — `arg_clause` / `pat_clause` / `stats_splice` (block statements) / the
template body and parents in `reify_defs` — with the caller passing the element lowering as a function
(arguments as terms, pattern arguments as patterns, block elements as statements, parents as parents).
Rank 2 (`...$xss`) is still refused by `hole` itself with the existing message.

#### Two more things fixed along the way

* **Empty `TypeTree`s inside an expansion**. For `q"val ff = $f"`, nsc's quasiquotes build a
  `TypeTree()` (a tree with no type). `expand.rs` refused it with
  `the expansion contains an empty TypeTree`. We now lower it to `TreeKind::Empty`, but only in the
  type position of a `ValDef`, and let the typer infer. **Only in that position**, because nowhere
  else does our AST have a tree meaning "infer this".
* **`_root_` did not resolve in term position**. There was a branch only in `import_path_syms`, so
  `_root_.scala.collection.immutable.List(…)` gave `not found: value _root_`. `type_ident` now
  resolves it to the root package.

#### Validation

`tests/fixtures/sv_impl.scala` + `sv_use.scala` are compiled in two stages and print 4 lines, and
**the same two files, compiled in two stages by the real scalac 2.13.16 and run, give the same 4
lines** (`tests/fixtures/expected/sv_use.txt`). The mixed splice in the template body puts **the
printed string of the tree it built** into the expansion, so if a splice landed in a different
position the line would change (while still compiling and running).
`sv_gaps_bad.scala` pins the 3 refused shapes (the real scalac refuses 2 of them too, so those pin
agreement).

### 7.17 Blocks, and members of static `object`s, in `reify`

Two of the shapes §7.15 left refused are now built, and both are the same rule seen twice:
**a reference is reified by the symbol it resolved to, never by the name that was written.**

#### 1. A term member of a static `object`

`reify { println("a") }` was refused with `` `println` is a local, a parameter, or a name that
does not stand for a static `object` ``. It is none of those: by the time nsc's reifier sees the
body its typer has already rewritten `println` to `scala.Predef.println`, and the tree it builds is

```text
$u.Select($u.internal.reificationSupport.mkIdent($m.staticModule("scala.Predef")),
          $u.TermName("println"))
```

(measured with `-Xprint:typer` on `reify { println("hello " + "world") }`). scala-rs builds the same
tree, from a new `ReifyRef::StaticMember`. What decides it is the **declaring owner**: the symbol the
name resolved to must be a `def` or a `val` whose owner is a module class reachable through packages
alone — the same test `static_module_name` already applied to an `object` reference itself. A member
of a class, a local, or a parameter fails that test and stays refused, because none of them can be
found again through a mirror.

A bare name is overloaded more often than not (`Predef` declares seven `println`s), and typing the
`Ident` on its own settles nothing. So the callee of an application is resolved by typing the
**whole application** speculatively and reading the head's symbol off the result
(`Check::applied_static_member`); the classification is then recorded against the node that was
*written*, which is what `crate::reify` asks about.

Two members of a static `object` were deliberately **refused** here, because nsc builds a different
tree for them and building this one instead would be wrong in a way only running the program shows
(the first is built since §7.26):

| Shape | What nsc builds |
| --- | --- |
| A member of the `object` that lexically **encloses** the `reify` (`object Impls { val x = 42; def foo(c: Context) = reify { x } }`) | `Select(mkThis($m.staticModule("Impls").asModule.moduleClass), TermName("x"))` — measured on `test/files/run/macro-reify-ref-to-packageless`. nsc's typer spells the reference `Impls.this.x`, and `mkThis` is not `mkIdent`: it prints as a different tree and keeps access to a `private` member |
| A member whose name is not a legal JVM identifier once encoded (a backquoted `` `a b` ``) | nsc escapes the rest (`$u0020`); `scala_rs_pickle::names::encode_method_name` does not, so the `TermName` would name a member that does not exist |

`scala.math`'s package-object functions (`math.max`) are refused for a third reason, and this one is
about scala-rs rather than about nsc: `prelude_text.rs` declares them on the **package** `scala.math`
with `Flags::STATIC`, not on a module class, so there is no static `object` to name. nsc writes
`staticModule("scala.math.package")`.

#### 2. Blocks

`reify { println("a"); println("b") }` was refused outright. nsc builds `Block(List(<init>), <last>)`,
which is exactly what `rs.SyntacticBlock(List(...))` (`gen.mkBlock`) produces, so the lowering the
quasiquote path already had is the right one — the only thing missing was letting a `Block` through
`Reifier::reify_term` and walking into it from `Check::reify_refs_in`. The block's own type is its
last expression's, and that is what `Expr.apply[T]` is instantiated at.

A definition inside the block was refused here; §7.19 found that nsc reifies a definition the body
binds for itself by name after all, and builds it.

#### Validation

* `tests/fixtures/rf_shapes.scala` — nine lines of `showRaw`, against the runtime universe: a block
  of two statements, a block whose last expression is a value, a nested block in argument position,
  `println`, an imported member applied and unapplied, and the same member selected on its `object`.
  **Real scalac 2.13.16 prints the same nine lines** (`tests/fixtures/expected/rf_shapes.txt`).
  Comparing the printed tree is the point: a bare `Ident(TermName("twice"))` compiles and evaluates
  to the same thing wherever a `twice` is in scope, and only the tree tells the two apart.
* `tests/fixtures/rf_impl.scala` + `rf_use.scala` — three macro implementations really expanded in
  two runs (`reify { println("hello " + s.splice) }`, the shape of
  `test/files/run/macro-reify-basic`; a two-statement block; a block whose value is the last
  expression, with a splice used twice). The last line prints how many times the argument's side
  effect ran, so a dropped or duplicated splice changes the output. Real scalac 2.13.16 gives the
  same seven lines.
* `tests/fixtures/rf_more.scala` — the bodies this section refused (formerly `rf_bad.scala`), which
  run since §7.26.

The tests are `crates/cli/tests/rf_reify.rs` -- its own file, since `reify.rs` is taken by an
unrelated suite about dispatching to the declaring class.

### 7.19 `val` and `def` definitions bound inside a `reify` body

§7.17 refused every definition inside a `reify { … }` body outright, reasoning from nsc's *free-term*
machinery: "nsc reifies it with `build.newNestedSymbol` and links every reference to that symbol.
Building the definition by name instead would compile and run, and would bind whatever name the
expansion site happens to use." Measured with `-Ymacro-debug-lite`, that reasoning does not apply to a
`val` or `def` the body binds *for itself*. `reify { val x = 1; x + 1 }` reifies as

```text
$u.Block.apply(List($u.ValDef.apply($u.NoMods, $u.TermName("x"), $u.TypeTree(), $u.Literal($u.Constant(1)))),
               $u.Apply.apply($u.Select.apply($u.Ident.apply($u.TermName("x")), $u.TermName("$plus")), List($u.Literal($u.Constant(1)))))
```

No `newFreeTerm`, no symbol at all: the binding is `$u.ValDef.apply(...)` and the reference is a bare
`$u.Ident.apply($u.TermName("x"))`, the same structural, by-name shape a quasiquote already builds.
This holds for a `def`'s own parameters too (`reify { (y: Int) => y + 1 }` reifies `y` the same bare
way), and for a `def` that calls itself or another `def` in the same block declared *after* it —
`reify { def isEven(n: Int) = ...isOdd...; def isOdd(n: Int) = ...isEven...; isEven(10) }` compiles and
reifies both directions by name, because nsc lets a block's `def`s see each other regardless of
textual order. **`build.newNestedSymbol` is nsc's own bookkeeping for telling a name bound inside the
tree being reified from one that is free with respect to it — not something that shows up in the tree
its reifier builds.** The free-term shape is needed only for a local or a parameter bound *outside*
the `reify` body, which §7.26 builds.

#### The declared type

Building `val x: Int = 1` surfaced a shape not measured before: **a written *value* type is not
reified the same way a type *argument* is.** `f[Int]` at a call site, or the `T` of `Expr.apply[T]`,
becomes `mkTypeTree(...)` around a `Type` built the way a `TypeTag` is (`crate::materialize::TagBody`,
§7.15) — but `def f(y: Int): Int = ...` reifies `y`'s type as

```text
$u.internal.reificationSupport.mkIdent($m.staticClass("scala.Int"))
```

— an `Ident` carrying a resolved *symbol*, exactly the shape a static `object` reference already gets,
built *structurally* the way a quasiquote builds a type, not embedded as a raw `Type` object. For a
parameterised type the difference is sharper: `List[Int]` reifies as

```text
$u.AppliedTypeTree.apply($u.Select.apply(mkIdent($m.staticModule("scala.package")), $u.TypeName("List")),
                         List(mkIdent($m.staticClass("scala.Int"))))
```

i.e. the whole type tree is walked structurally and only each *leaf* naming a class or a module member
is resolved by symbol — the same rule the rest of reification already follows, applied one level down.
`Reifier::typ`'s ordinary, non-reify branch is what a quasiquote uses for this, and reusing it
verbatim would reify the leaf by the written name, not by symbol. The two builders must not be confused: reusing the type-*argument* builder for a value
type compiles, runs, and looks plausible (`TypeTree()` instead of `Int`'s `Ident`, or a wrong wrapped
`Type` for `List[Int]`) — only comparing the printed tree against real scalac 2.13.16 catches it
(`tests/fixtures/rd_defs.scala`).

This section first built only the single-leaf case (one class reachable through `staticClass`);
since §7.26 a written value type of any shape is built by the typed reifier's type reifier.

#### Validation

* `tests/fixtures/rd_defs.scala` — `showRaw` of six reified trees (an untyped `val`, a typed `val`, a
  `def` with a typed parameter, a recursive `def`, two mutually recursive `def`s, and a `val` read by a
  `def`) against the runtime universe. **Matches real scalac 2.13.16 exactly** (`rd_defs_match_real_scalac`).
* `tests/fixtures/rd_defs_valimpl.scala` + `rd_defs_valuse.scala` — the `val` case really expanded
  through the JVM bridge in two runs and executed, matching real scalac's own two-stage run.
* `tests/fixtures/rd_defs_typed.scala` — the written types this section first refused
  (formerly `rd_defs_bad.scala`), now built (§7.26).

The tests are `crates/cli/tests/reifydefs.rs`.

### 7.20 Reverse RPC: `c.typecheck`, `c.inferImplicitValue`, and the current run's symbols

A macro implementation sometimes needs the compiler: `c.typecheck`, `c.inferImplicitValue`, or the
members of a class the current run is compiling (§5.1). All three are the same piece of work, a
channel on which the engine asks the Rust typer and waits for the answer.

#### The channel

Expansion used to be one line out and one line back: scala-rs wrote `(expand …)` and read the reply.
It is now a **conversation**. An implementation running inside the engine may stop mid-flight, write
`(q …)` on the same stdout the reply would go to, and block reading its own stdin; scala-rs — which
is sitting in `MacroEngine::read_reply` waiting for that reply — sees the `q`, answers `(a …)` or
`(no "reason")`, and goes back to waiting. The two processes take turns, so the pipe stays in step:
exactly one line written for one line read, at every level.

```text
scala-rs (Rust)                        engine (JVM)
───────────────                        ────────────
(expand …)               ──────→       invoke the implementation
                                          c.typecheck(Ident(TermName("x")))
                         ←──────       (q typecheck …)
type it, here, now, in the
scope the macro was called from
                         ──────→       (a ok <type> <tree>)
                                       … the implementation goes on
                         ←──────       (ok <expansion>)
```

The pieces are `Typer::converse` / `converse_with` (`crates/typer/src/expand.rs`), which drives the
loop, and `Typer::answer_query` (`crates/typer/src/expand_rpc.rs`, new), which answers. The engine
side is `ScalaRsMacroEngine.query`.

**Why it has to be a channel and not a message.** The thing an implementation wants to know — what
does this tree mean *here* — depends on the scope the macro was called from, and that scope has only
ever existed inside scala-rs. Sending a description of it up front is what §5.1 shows to be
impossible. Asking instead forces exactly the lazy signature the typer would have forced, at the
moment the question is asked.

**Three answers, never a fourth.** `(a ok …)`, the real answer; `(a fail "msg")`, "the typer rejected
that", which the engine raises the way nsc does; and `(no "reason")`, *scala-rs cannot answer this
question*, which becomes the call site's diagnostic. A question scala-rs would have to guess at is
the third kind. There is no "approximately".

#### Re-entrancy, timeouts and cycles

Three failure modes had to be closed, and each of them is a thing that has actually gone wrong in
this project before.

* **Re-entrancy.** A reverse query can expand another macro on the same JVM. The typer keeps
  the engine handle available while answering, and the JVM query loop services nested expansion
  requests before receiving the answer to the outer query. Each nested request saves and restores
  the outer transport's splice maps, structural types, pending failures and active contexts. Symbol
  identities and fresh names remain shared. Protocol errors or unwinding still poison the engine.

* **The timeout now measures the implementation's own time.** `SCALA_RS_MACRO_TIMEOUT_SECS` is
  unchanged at 20 seconds, but only the intervals during which *the engine holds the ball* are
  subtracted from it (`MacroEngine::read_reply` takes a `&mut Option<Duration>` and decrements it by
  what the read cost). An implementation that asks a hundred questions must not be killed for how
  long scala-rs took to answer them. That leaves a macro that asks without end, which no time budget
  catches because answering costs it nothing: `MAX_ENGINE_QUERIES` (1024 per expansion) does, and
  `MAX_QUERY_DEPTH` (64) bounds a chain of nested questions, allowing product derivations that
  recursively request evidence for each field. Implicit query results are checked for unexpanded
  macro calls throughout the returned tree, so failed nested evidence cannot escape as a successful
  result and be retried under the enclosing macro's implicit context. The separate `(timing)` round trip has
  its own two-second deadline. A timeout or framing failure poisons the engine and terminates its
  owned process tree before another expansion can reuse the pipe. Unix uses a dedicated process
  group; Windows creates the JVM suspended, assigns it to a kill-on-close Job Object, and only then
  resumes its primary thread. Termination ownership is consumed once, so a rejected startup hello cannot
  make `Drop` signal a reaped process's stale PID or process-group id.

* **Cycles.** A class that refers to itself must not make a description recurse. Since §7.25 a
  current-run class's info is a lazy type completed on demand, so a reference back to a class that is
  being completed closes over the symbol the engine already has. `class Node { def next: Node }` is
  exactly this, and it is in the fixture: `t.tpe.member(TermName("next")).info` prints `Node` under
  scala-rs and under real scalac alike.

#### `c.typecheck`

`c.typecheck(tree, mode, pt, silent, withImplicitViewsDisabled, withMacrosDisabled)` in **TERMmode**
and **TYPEmode**. The tree is serialised with the engine's existing generic tree serialiser, rebuilt
by `Typer::tree_from_reply`, typed by the real `Typer::type_expr` (or read by `tree_to_type` in
TYPEmode) at the call site, and written back with its type.

Diagnostics the attempt raises are **rolled back**, not reported: a `c.typecheck` that fails is not a
compile error in nsc either — it raises `TypecheckException` and lets the implementation decide — and
an implementation that probes with `silent = true` must not leave errors behind on the way.

The four things this bridge cannot honour are refused **by name** rather than ignored, because
ignoring any of them answers a question the implementation did not ask:

| asked for | why it is refused |
| --- | --- |
| a `pt` other than `WildcardType` | scala-rs types the tree with no expectation; a `pt` changes what the answer may be |
| `withImplicitViewsDisabled` | scala-rs's typer has no switch for it |
| `withMacrosDisabled` | likewise |
| `PATTERNmode` | reading it as TERMmode would type a pattern as an expression |

**`TypecheckException` cannot reach an implementation at all.** This is a limit of the execution
model, and it is worth writing down because it is not obvious: the
`Context` is a `java.lang.reflect.Proxy`, a proxy wraps any *checked* exception the interface method
does not declare in an `UndeclaredThrowableException`, `TypecheckException extends Exception`, and
`Typers.typecheck` declares nothing. So `catch { case c.TypecheckException(_, msg) => … }` — the way
the failure is meant to be handled, and what `t6814` and `macro-typecheck-implicitsdisabled` both
write — does not match. An implementation that catches it therefore decided what to do next on
something it was not told, so the expansion is **refused with that reason** rather than accepted.
Closing this needs a generated `Context` class instead of a proxy, which is a much larger change than
it sounds: it means writing all 72 members out by hand in Java, against a Scala interface.

#### `c.inferImplicitValue`

`c.inferImplicitValue(pt, silent, withMacrosDisabled, pos)` asks the Rust typer to run its real
implicit search in the macro call site's live scope. A miss returns `EmptyTree`; a found witness is
materialised and adapted exactly as an omitted implicit argument is. Speculative diagnostics are
rolled back when `silent` is true. `withMacrosDisabled` is applied while candidate sets are built,
including recursive searches, rather than removing a winner after the search has already run.

The requested type wire preserves abstract-member identity and a stable singleton prefix:
`Tracer.instance.Type` is sent as its `Tracer.Type` declaration plus the `Tracer.instance` prefix.
Returned trees likewise rebuild compound type trees such as
`Tracer.instance.Type with Tracer.Traced`, preserving the prefix while the expansion is typechecked
at the call site. A local, parameter, or other non-static prefix the wire cannot identify is refused;
it is never searched as the bare declaration, because `p.Type` and `q.Type` are distinct types.
Unsupported result types are likewise refused rather than widened to a class name.

An enabled implicit macro is selected by the ordinary search and expanded through a nested
conversation on the same JVM. Failed expansions are reported rather than returned as unexpanded
implicit references. A `silent = false` miss leaves the typer's missing-implicit diagnostic at the
call site; all speculative type-resolution and adaptation diagnostics are rolled back on every
other exit. The `macronestedquery` regression compares nested implicit materialization with scalac,
including outer context restoration and a side-effecting argument splice.

The default fourth argument is the enclosing macro-call position. The bridge verifies that the
received `Position` equals that value; a different explicit `pos` is refused by name because the
Rust side currently has no faithful position wire and silently substituting the call site would
attach diagnostics to the wrong source location.

#### The current run's symbols

A class the current run is compiling has no class file on the macro classpath, so the engine's
mirror cannot find it (§5.1). The reverse channel is what lets the engine ask about it instead. The
first version sent an eager description, `(run "a.b.C" (f …) (parents …) (decls …))`; §7.25
replaced it with a lazily completed symbol in nsc's shape.

One rule carried over unchanged, and it is the whole design: **either scala-rs can describe the
class truthfully, or the question is refused.** A `decls` missing a member is not less information;
it is the *wrong answer* to `decls`, and an implementation that acts on it builds a tree from a class
it half understands. A half-built mirror is worse than a refusal, so what cannot be translated
faithfully is refused by name (§7.25 lists the shapes).

Flags travel **by name** (`CASE`, `TRAIT`, `ABSTRACT`, `FINAL`, `SEALED` on a class; `DEFERRED` and
the access flags on a member), looked up on `universe.Flag` at the far end, for the same reason the
`Modifiers` serializer argues in the other direction (§7.13): nsc's bit layout is an internal detail
and several bits carry two names.

#### Validation

* `tests/fixtures/mtc_impl.scala` + `mtc_use.scala` — eight `c.typecheck` questions, expanded for
  real through the bridge and **executed**, byte-identical to real scalac 2.13.16 compiling the same
  two files against each other (`crates/cli/tests/macromirror.rs`,
  `mtc_typecheck_expands_and_runs`). Two of the eight are the mirror: `Marker` and `Node` are classes
  *this compilation is defining*, so `mirror.staticClass` could never find them. One is the constant
  type `Int(1)`, which had to travel as a constant type rather than be widened. One splices the tree
  `c.typecheck` returned straight into the expansion, so what runs is what the answer said.
* `tests/fixtures/mtc_bad_impl.scala` + `mtc_bad.scala` — questions the bridge cannot answer, each
  refused with a reason that names the missing capability, most of them in programs real scalac
  compiles and runs. `mtc_unanswerable_questions_are_named` pins each refusal's wording.
* `tests/fixtures/miv_impl.scala` + `miv_use.scala` — path-dependent and ordinary
  `c.inferImplicitValue` queries plus a ZIO-shaped compound-type expansion. The implementation is
  compiled once by real scalac; scala-rs and scalac then compile and execute the same use site, and
  `infer_implicit_value_matches_real_scalac` requires identical output.
* `miv_enabled.scala`, `miv_nonstatic.scala`, `miv_position.scala`, and `miv_required.scala` — an
  enabled implicit macro expands during the outer query, a parameter-dependent prefix is refused
  without an approximation, a non-default diagnostic position is refused rather than ignored,
  and a non-silent miss produces exactly one diagnostic. Real scalac is the positive oracle for
  the first three and the failing oracle for the fourth.
* `miv_zio_actual.scala` compiles `ZIO.succeed(1)` against ZIO 2.1.26's real
  `zio-stacktracer` artifact and requires the resulting `Main$.class`. This complements the
  structurally equivalent local fixture with the exact `autoTraceImpl` that motivated the RPC.

#### Known limits

1. **`c.inferImplicitView`** is not implemented. It needs conversion search and a faithful
   representation of both requested endpoint types; it is not an alias for `inferImplicitValue`.
2. **`TypecheckException` cannot reach an implementation** while the `Context` is a
   `java.lang.reflect.Proxy` (see "`c.typecheck`" above). Everything else the proxy does is fine;
   this one checked exception is the whole cost.

### 7.21 A type tag that carries type arguments, and the `Expr[Nothing]` nsc really passes

The tag descriptor on the wire originally carried a class name and nothing else, so a request whose
tag was an applied type constructor could not be built, and every `mapTo` stopped there:

```text
cannot expand mapTo (implementation slick.lifted.ShapedValue$.mapToImpl): scala-rs cannot
build a type tag for `ClassTag`, a type constructor applied to type arguments
```

The descriptor now carries type arguments, and — validating that against real scalac — the tag it
was failing to build turned out to be **a tag nsc never builds**.

#### The descriptor

`Typer::tag_wire` (`crates/typer/src/expand.rs`, replacing the body of `tag_descriptor`) writes
`(ty "a.b.C" <arg>…)` and recurses into the arguments. The engine side needed nothing: `typeFor`
already read the shape and applies it with `universe.appliedType` — checked rather than assumed.

Three of scala-rs's own `Type` variants are not class applications and had to be written out,
because nsc's tag for each *is* one: `Type::Tuple(ts)` is `scala.TupleN[ts…]`,
`Type::Function { params, ret }` is `scala.FunctionN[params…, ret]` and `Type::Array(t)` is
`scala.Array[t]`. A constant type nested inside an argument stays a constant (`Tag[1]` is not
`Tag[Int]`); only the outermost type is widened, which is what nsc does for the type it gives an
`Expr`.

A class **this run is compiling** travels by identity (§7.25), applied to type arguments or not, and
so does a type parameter of the implementation's weak tag. What `tests/fixtures/gbm_bad.scala` still
pins as refused is a tag for the singleton type of a source object.

#### The `Expr[Nothing]`, which is the part that was not expected

`tests/fixtures/gbm_use.scala` runs an implementation with slick's own opening — `isCaseClass`, then
every case accessor's `typeSignature` — and prints its `c.Expr`'s `staticType`. Real scalac
2.13.16 prints **`Nothing`**.

That is not an accident of the fixture. nsc's `Macros.macroArgs` wraps every value argument as

```scala
case LiftedTyped => context.Expr[Nothing](duplicatedArg)(TypeTag.Nothing) // TODO: SI-5752
```

so an argument's tag is a **constant** — the same constant `c.prefix` gets, and for the same
reason. scala-rs was building a tag for the argument's real type instead. That is a silent
divergence in its own right (an implementation reading `arg.staticType` would be told
`ClassTag[Row]` here and `Nothing` by nsc), and it is *also* why gitbucket's 31 sites were refused:
`ShapedValue.mapToImpl` takes a `c.Expr[ClassTag[R]]`, and scala-rs was refusing the expansion
because it could not build a tag **nsc does not build at all**. The request now sends
`(ty "scala.Nothing")` for every `Expr` argument, and `gbm_use.scala` pins the `Nothing` against
scalac's own output.

The descriptor is still needed, and is still exercised, for the `(tags …)` clause — the
`c.WeakTypeTag[T]`s an implementation really does ask for.

#### A named difference: aliases

`weakTypeOf[Map[String, List[Int]]]` prints `Map[String,List[Int]]` under nsc and
`scala.collection.immutable.Map[String,List[Int]]` under scala-rs; `Either` behaves the same way
through `scala.package.Either`. **The types are the same type** — they answer `=:=` and every
question an implementation asks of them identically — but scala-rs expands an alias away long
before a type reaches the tag descriptor, so its tag names the class where nsc's names the alias,
and nsc's printer omits the prefix of an alias owned by `scala` or `Predef` while it does not omit
`scala.util.` or `scala.collection.immutable.`.

An implementation that *prints* or string-matches a tag therefore sees a different spelling. This
is stated here, and both spellings are asserted in `gbm_applied_tags_match_real_scalac`, rather
than being hidden by leaving the two cases out of the fixture. Closing it needs scala-rs to keep
alias types, which is a type-checker decision and not a macro one.

#### Validation

* `tests/fixtures/gbm_impl.scala` + `gbm_use.scala` — nine tags whose types carry arguments,
  expanded for real and **executed**. Eight of the ten output lines are byte-identical to real
  scalac 2.13.16 compiling the same two files against each other; the two that differ are the alias
  difference above, and the test asserts *both* spellings
  (`crates/cli/tests/gbmapto.rs`, `gbm_applied_tags_expand_and_run` and
  `gbm_applied_tags_match_real_scalac`). The first line is slick's `mapToImpl` as far as it reads
  its type argument, driven by a tag that could not previously be built.
* `tests/fixtures/gbm_bad.scala` — the source singleton tag scala-rs still refuses, by name, in a
  program real scalac compiles and runs.
* `tests/fixtures/eg_gaps_bad.scala` lost one case and gained another: `nameOf[List[Int]]` was a
  pinned *refusal* there and is now a pinned acceptance in `gbm_use.scala`, so its place is taken by
  `nameOf[Main.type]`, a singleton type, which is still refused. Real scalac still compiles and runs
  that file.

### 7.22 The type arguments written on the macro implementation reference

The type arguments written on a macro implementation reference are read, in both readers (the pickle
and the source), and resolved the way nsc resolves them.

#### Lining tags up with the call site was never going to work

`def mapTo[R] = macro ShapedValue.mapToImpl[R, U]`. `R` is `mapTo`'s own type parameter and `U` is
`ShapedValue`'s, so the implementation asks for two `WeakTypeTag`s where `mapTo[Account]` writes one
type argument. nsc does not line the two up at any point. `Macros.macroArgs` reads
`MacroImplBinding.targs` -- the type arguments written on the *implementation reference* -- and
resolves each one on its own:

```scala
val targ = binding.targs(paramPos).tpe.typeSymbol
val tpe = if (targ.isTypeParameterOrSkolem) {
  if (targ.owner == macroDef) targs(macroDef.typeParams.indexWhere(_.name == targ.name)).tpe
  else targ.tpe.asSeenFrom(if (prefix == EmptyTree) macroDef.owner.tpe else prefix.tpe, macroDef.owner)
} else targ.tpe
context.WeakTypeTag(tpe)
```

`paramPos` is not a flag: the non-negative fingerprint nsc pickles for a tag parameter **is the
index** of the implementation type parameter that tag is for, which is also the index into
`binding.targs`. scala-rs counted those fingerprints and threw the values away
(`macro_signature_shape`), and threw the type arguments away twice over -- `PickleReader::macro_impl_of`
peeled the `TypeApply` off the annotation payload, and `macros.rs::split_type_apply` reduced the
source-side reference's type arguments to a count.

**The old rule was not merely incomplete, it gave wrong answers silently.** With two written type
arguments and two call-site ones the counts matched and the tags went over in call-site order:
`def swapped[A, B] = macro Impl.pairImpl[B, A]` called as `swapped[Int, String]` printed
`R=Int U=String` under that rule and prints `R=String U=Int` under real scalac 2.13.16.
`tests/fixtures/mt2_use.scala` pins the right answer.

#### The three pieces

* **The pickled reader.** `MacroImpl::targs` (`crates/pickle/src/sym.rs`) is the list nsc wraps the
  `@macroImpl` payload in, read out of each type-argument tree's own pickled type. A tree with no
  type is kept as `SigType::None` rather than dropped, so the fingerprint indices into the list stay
  valid. `PickleReader::macro_impl_targ_trees` matches the `TypeApply` at the top and nowhere
  deeper, which is exactly what nsc's `MacroImplBinding.unpickle` does.
* **The source reader.** `split_type_apply` hands back the type-argument *trees*, and
  `Typer::classify_macro_targ` matches a bare name against the macro def's type parameters and then
  its owner's **by name**. By name is both what nsc does and the only thing available: a macro def's
  right-hand side is resolved in the *enclosing* scope with no parameter scope pushed -- the
  reference names a method of some other object and must not see `R` as anything -- so typing `R`
  there answers "not found: type R".
* **The classification.** Both readers end in `macros.rs::macro_targ_of_type`, which decides once,
  where the binding is made, which *kind* of thing each written type argument is
  ([`MacroTarg`]): a type parameter of the macro def, a type parameter of its owner, a type written
  out in full, or one this bridge refuses. What each one *stands for* is decided at every call site
  by `Typer::tag_types` (`crates/typer/src/expand.rs`), where the call's type arguments and its
  prefix are: `OwnerParam` is `SymbolTable::base_type_args` on the receiver's type, which is
  `asSeenFrom` for a class type parameter, so `SubShaped[Char]` is seen as `Shaped[Char]` first.

`MacroBinding::tag_targs` being **empty means "not known"**, not "no tags": a binding whose reference
could not be read this way keeps the older one-for-one rule, which is right for the usual
`macro Impl.f[A]` and is refused with a reason when it does not fit. Nothing that expanded before
stops expanding.

#### What is refused, and why nsc's own answer is not a reading

nsc reads `binding.targs(i).tpe.**typeSymbol**` and then *that symbol's* own type. For a bare type
parameter or a type with no arguments that is the same type back again. For an applied type
constructor it is not: `macro Impl.pairImpl[List[R], Int]` hands the implementation `List[A]`, `A`
being `List`'s own type parameter, related to nothing at the call site. Real scalac 2.13.16 prints
exactly that (`tests/fixtures/mt2_bad.scala` is the program, and it compiles and runs).

scala-rs refuses it by name. Copying nsc would mean handing an implementation a type with a free
parameter in it; substituting instead would answer `List[String]`, which is a different answer from
the compiler this project is a compiler for. The same goes for a type parameter of the owner with
**no receiver** to see it through: nsc falls back to `macroDef.owner.tpe`, so `U` reaches the
implementation as the free `U`, and scala-rs says so instead of guessing.

#### A macro def read from a class-file directory

A macro def whose owner is a **class** reached through a class-file *directory* on `-cp` used to be
installed by the eager flat pickle reader as an ordinary method returning `Any`, **with no
`MacroBinding` at all**, so the call compiled into a call to a method that does not exist
(`NoSuchMethodError`). That reader now skips `MACRO` members and leaves them to the full supplier
(§7.23). `crates/cli/tests/mapto2.rs` still packs `tests/fixtures/mt2_mdef.scala` into a **jar**,
which is what slick and gitbucket are.

#### Validation

* `tests/fixtures/mt2_mdef.scala` -- compiled by **real scalac** (only nsc writes the `@macroImpl`
  annotation) and packed into a jar -- plus `tests/fixtures/mt2_use.scala`, seven implementation
  references expanded for real and **executed**, byte-identical to real scalac 2.13.16 compiling the
  same two files against each other (`crates/cli/tests/mapto2.rs`,
  `mt2_reference_targs_expand_and_run` and `mt2_reference_targs_match_real_scalac`). Six come out of
  the jar's pickle and one is a macro def compiled in the same run, so both readers are covered.
  Among them: slick's own shape (`R` from the call site, `U` from the receiver), a receiver whose own
  argument is applied, the owner's parameter reached through a *subclass*, a reference that writes
  the macro def's parameters in the other order, a type argument written out in full, and an
  implementation whose tag clause is in the other order from its type parameters.
* `tests/fixtures/mt2_bad.scala` -- three references scala-rs refuses, each by name, through both
  readers. **All three are a program real scalac compiles and runs** (`R=List[A] U=Int` twice and
  `R=Boolean U=U`); scala-rs accepts none of them.

### 7.23 Structural transport

The engine protocol carries blocks, local
methods, functions, conditionals, type ascriptions, constructor applications,
repeated argument groups, and concrete local classes with superclass
constructor arguments. Every reconstructed node receives a fresh identity;
sharing NodeId(0) let an earlier block-local declaration hide a later class
member. Class templates retain initialization statements and constructor
parameters, and superclass applications retain their arguments. (Type-definition
bounds were refused at first; §7.25 rebuilds `TypeDef` / `TypeBoundsTree`.)

`c.typecheck` preserves attachments when adapting a tree and rejects unresolved
TERM overloads before serialization, so `silent = true` returns EmptyTree as
scalac does. `c.untypecheck` rebuilds trees while clearing local bindings/types
and retaining external bindings. Source paths, source text and UTF-16 call-site
points now reach the JVM engine for `macroApplication.pos` and
`enclosingPosition`. Repeated Expr/Tree arguments are packed using the macro
declaration's repeated parameter slot, including an empty repeated group.

Binary macro supply retains access flags and boundaries. The eager flat pickle
reader skips macro declarations: they have no JVM method, and only the full
supplier can attach their implementation metadata. Qualified reflection calls
materialize tags in their receiver's universe without requiring a wildcard
import; this closes the missing WeakTypeTag operand in inferred `c.Expr` bodies.

`macrotransportbatch` compares both API producers and both consumers against
scalac 2.13.16, executes with `java -Xverify:all`, and checks independent access
and argument-type rejections. Former unsupported block/function/new cases from
`engine` now have executable comparisons, including constructor side-effect
counts; the singleton-tag and receiverless-prefix refusal checks remain.

### 7.24 Source symbol ownership

Source definitions and typed functions now retain symbol identities across the
engine protocol. `c.internal.enclosingOwner` follows lexical initialization
owners, including an inferred val inside a method. Function parameters belong
to the anonymous function symbol. The JVM mirror completes symbol info by
reverse RPC when requested; an inferred val forced during its own expansion
reports the recursive-value error rather than acquiring a fabricated NoType.
The anonymous function's own NoType matches nsc's typed tree representation.

Both `c.internal.changeOwner` and `c.universe.internal.changeOwner` run Scala's
real ChangeOwnerTraverser. Ordinary scala-reflect runtime symbols disallow owner
mutation. For fresh source symbols, the bridge copies the loaded synchronized
symbol implementation under a private class name and supplies an owner setter
that records originalOwner and updates only that source symbol. The runtime's
synchronization mixins and constructor checks remain intact; loaded classpath
symbols retain their original implementation. Unsupported runtime layouts or
symbol kinds produce diagnostics. This adapter is exercised with 2.13.16, not
claimed compatible with arbitrary scala-reflect versions.

Repeated typechecks retain definition identities and restore hoisted definitions
to the new block scope. Local class types use NoPrefix when their lexical owner
is a method or val; ThisType requires a class owner. Macro stdout and stderr
travel as explicit protocol messages, so println cannot be mistaken for a tree
reply. The ownership fixture checks both output channels, UTF-8 text, both
changeOwner entry points, function/parameter identity, and recursive owner-info
rejection across both API producers and both consumers.

Polymorphic source-symbol info and source class shapes that cannot be fully
described are explicitly refused.

### 7.25 `mapTo` against classes this run is compiling

gitbucket's `(a, b).mapTo[Row]` call sites name a row class the same run is compiling, so the
engine's mirror cannot find it (§5.1), and `mapToImpl` interrogates it thoroughly:
`rSym.asClass.isCaseClass`, `rSym.companion`, that companion's `tupled`, `rTag.tpe.decls` and each
accessor's `typeSignature`. Its result is a `Block` of quasiquotes containing an anonymous class with
`override def`s, `Match` / `CaseDef` / `Bind` patterns, `New` with type arguments and `Super`. Both
halves -- describing the class, and rebuilding the result -- are below, and every `mapTo` in
gitbucket expands.

A standalone program proves the whole path: `tests/fixtures/gbmac_mapto.scala` declares four case
classes and their tables in the shape gitbucket writes them -- the tables in a component trait with a
self type and an imported profile API, two `mapTo`s in one class -- and taking each of
`mapToImpl`'s branches (companion with `tupled`; one field; explicit companion without `tupled`; a
class implementing a trait's abstract `val`s, with a default, `Option` and timestamp fields). Built by
scala-rs and by real scalac 2.13.16, both run against in-memory H2 under `-Xverify:all` and print the
same rows, the same `toMapped` / `toBase` results and the same fast-path converter names
(`Fast Path of (String, Int, Option[String]).mapTo[gbmacm.Account]`).

`tests/fixtures/gbmac_prefix.scala` isolates the remaining transport edge: two inner `mapTo`s feed an
outer `mapTo` in a local table, so the outer implementation reads the generated converter through
`c.prefix`. Both scala-rs and real scalac compile and run it, printing the same `prefix` result.

#### 1. A class this run is compiling travels as its identity, and is described lazily, in nsc's shape

A type tag for a current-run class is now `(src <id>)`: the class's scala-rs symbol id. The engine
builds its symbol (`ScalaRsMacroEngine.sourceSymbol`) with a `LazyType` info that asks scala-rs
`(q symbolInfo <id>)` only when the implementation forces it -- the reverse channel of §7.20, as
§5.1 calls for. `TableQuery[Issues]` never forces it and costs nothing;
`mapTo[Account]` forces the class, its companion, and the case fields' types, and nothing else. A
`val fontColor = { … }` in the row class is never inferred on the macro's behalf, which is nsc's own
order of evaluation.

The symbol is built under the class's **real owner**: a package is the runtime mirror's own
package of that name (`mirror.staticPackage`), and the class and its companion are entered into its
scope. nsc finds a companion by looking the name up in the owner's declarations
(`Symbol.companionModule0`), so that is the only way `rSym.companion` can be right; the runtime
package scope answers entered names before it falls back to class loading. An object's module and
module class are made once, together (`(q modulePair …)`), and a class and its companion bring each
other in (`(q companion …)`).

What the class *is* is translated from scala-rs's model into nsc's (`crates/typer/src/expand_mirror.rs`):

* a `val` is one symbol here and two in nsc -- a `private[this]` field whose name ends in a space and
  a getter, plus a setter for a `var`; a constructor parameter that is not a `val` is the field alone,
  without the space; a `lazy val`, a trait's `val` and an abstract `val` are accessors alone. Each
  gets nsc's flags (`PARAMACCESSOR`, `CASEACCESSOR`, `STABLE`, `ACCESSOR`, access, `final`,
  `implicit`), and each view's type is asked for separately (`(q viewInfo getter|setter|field …)`);
* a constructor returns the class, with its parameters' names and `DEFAULTPARAM`s; a trait has a
  `$init$` unless it is a pure interface (`<interface>`), and is `abstract`;
* a case class lists nsc's synthetic members in nsc's order, with nsc's flags and parameter names --
  including `productIterator`, `hashCode`, `toString` and `equals`, which scala-rs leaves to the
  backend -- unless the class or a source ancestor declares one; a default getter's result is
  `@uncheckedVariance`;
* a case companion lists `<init>`, `apply` and `unapply` with the class's parameters, and the
  synthetic companion's `toString` or the explicit one's `writeReplace`; a synthetic companion
  extends `AbstractFunctionN` alone (so `tupled` is found exactly when nsc finds it), an explicit one
  is `Serializable`.

`tests/fixtures/gbmac_decls_use.scala` asks a macro (`gbmac_decls_impl.scala`) for all of that on
eleven classes -- case classes with vars, defaults, body `val`s, `lazy val`s and `private[this]`
fields, an explicit companion, a plain class, a trait with every kind of `val`, a pure trait,
abstract members, a subclass -- and scala-rs prints the same 231 lines as real scalac, flag for flag.

**What cannot be translated faithfully is refused**, naming the member (`gbmac_decls_bad.scala`,
which real scalac compiles): a nested class or object, a type member, a case class with a second
parameter list, a member with a qualified access boundary (`private[p]` / `protected[p]`: nsc keeps
`p` as the symbol's `privateWithin`, which the wire has no field for, and neither PRIVATE nor public
would be its answer), and a case class inheriting from a class-file ancestor, which might declare a
member that stops nsc synthesising one. Two differences are known and not refused: a `final val x = 3` is
`Int(3)` in nsc and `Int` here, and an `override def toString = …` written without parentheses is
`(): String` in nsc and nullary here -- both are what scala-rs's typer models, not the mirror.

This replaced both the eager `(run …)` description of §7.20 (which refused every class with a
field) and the name-only placeholder such a class used to travel as. `mg_inspect_bad.scala`'s `MgPlain
must be a case class` is now the program's own error, exactly as real scalac reports it, and
`gbmac_caseinfo_use.scala` prints what scalac prints for a current-run case class.

#### 2. Rebuilding what `mapToImpl` returns

`Typer::tree_from_reply` (`crates/typer/src/expand.rs`) now rebuilds, into the shape our parser
produces for the same source:

* `TypeTree`s with type arguments, as the path their class names applied to the arguments; a class
  this run is compiling is recognised by its full name and handed back resolved, wherever it is
  nested. The engine writes a `TypeTree`'s type only when its class has a path
  (`ScalaRsMacroEngine.serType`); anything else -- a singleton, a refinement, an abstract type --
  is `(tyx …)` and refused by name rather than read as the underlying class;
* `Match` with an empty selector -- nsc's `{ case … }` -- as `x$pf => x$pf match { … }`, with
  `CaseDef`, `Bind`, extractor and typed patterns, wildcard type arguments (`Bind(TypeName("_"))`),
  guards, alternatives and sequence wildcards (the last two only in a pattern, where they mean
  something);
* `Super`, `NamedArg`, `ExistentialTypeTree` and `TypeDef` / `TypeBoundsTree`, `DEFERRED` only
  on a `TypeDef`;
* a class with constructor parameters: nsc's `PARAMACCESSOR` field in the template body is folded
  into its parameter as `val`, `var` or a plain parameter.

`tests/fixtures/gbmac_shapes_impl.scala` returns each of these without slick, and both compilers'
builds print the same (`gbmac_shapes_use.scala`).

#### 3. The receiver and arguments go back typed

`c.prefix` of a `mapTo` is `anyToShapedValue(…)(Shape.tuple4Shape(Shape.repColumnShape(…), …))` --
the implicit view and its implicit arguments the typer inserted. It used to be rebuilt from its
shape and typed again at the call site, and typing it again resolved differently from the first
time in three separate ways: the typer writes an implicit found in a companion's implicit scope as a
bare `repColumnShape` once it is cached; a member reached through a component's self type as
`gitbucket.core.model.Profile.profile`; and `repColumnShape(dateColumnType)` typed as an explicit
application fails where the implicit search accepted it (`BaseColumnType[Date]`).

nsc hands a macro typed trees and splices them back without typing them again, and now so does
scala-rs. The receiver and each argument are sent as `(orig K <tree>)`; the engine remembers the
object it built for each (`origTrees`), and a reply containing that very object writes
`(t "Orig" (s0) K <tree>)`; scala-rs puts its own typed tree there, marked
`NodeId::PRETYPED_SPLICE`, which `Typer::type_expr` only adapts. A second mention of the same tree is
rebuilt from its shape, so no typed subtree is shared, and a tree the implementation changed is a
different object and is rebuilt and typed as before. The shape still travels for the implementation
to inspect, and for that and the fallback the outbound serialiser now writes a static object and a
member of one from `_root_`, a `classOf` the typer materialised as `Predef.classOf[T]` with `T` a
described type, and `C.this` for the enclosing class a member is reached through, its self type
included (`this_qualifier_of`, `class_path_member`).

#### 4. Three typer repairs the expansion needs

Each is shown without the macro in `tests/fixtures/gbmac_typer.scala`, compared with real scalac,
with its near misses in `gbmac_typer_bad.scala`.

* **An inherited alias to an abstract projection** (`crates/typer/src/check_types.rs`,
  `type_member_here`): `type Reader = M#Reader` seen from `class L extends Conv[IntDomain, …]` was
  returned unsubstituted, because an alias whose right-hand side is *another* type member was taken
  for a deferred member standing for itself.
* **A case class's `copy` from a jar** (`crates/pickle/src/sym.rs`, `Member::is_case_copy`): nsc
  pickles `apply` and `unapply` `CASE | SYNTHETIC` but `copy` only `SYNTHETIC`, so it was dropped
  with the synthetic plumbing and the class file's `copy(x$0, …)` stood in -- `super.getDumpInfo.copy
  (name = …)`, in every `mapTo` expansion, was "unknown parameter name: name".
* **A type member a jar class passes to a subclass** (`crates/typer/src/pickle_supply.rs`,
  `complete_inherited_type_member`, and `check_name.rs`): slick's `ResultConverter` declares
  `protected[this] type Reader = M#Reader`; no bytecode records an alias, and a bare `Reader` in a
  subclass body was "not found". It is now looked up through the enclosing classes' binary ancestors
  when nothing else binds the name, installed on the declaring class in its own vocabulary, and
  substituted through the subclass by the repair above.

#### Validation

`crates/cli/tests/gbmac.rs`, each fixture run under scala-rs and under real scalac:
`gbmac_mapto` on H2; `gbmac_mapto_bad` (`mapTo` to a class whose field types do not match, to one
with a column too many, and to a non-case class -- rejected on the same three lines by both
compilers); the mirror against nsc and its named refusals; `mapToImpl`'s opening on current-run
classes; the reply shapes; the three typer repairs and their near misses; `gbmac_selfimport` (below).

#### The receiver of a self-type member (gitbucket's component shape)

Every gitbucket table lives in `trait XComponent { self: Profile => import profile.api._ … }`, and
`gitbucket.core.model` has an `object Profile` beside `trait Profile`. Three receivers were silently
wrong there (`tests/fixtures/gbmac_selfimport.scala`; scalac prints what
`expected/gbmac_selfimport.txt` holds):

* **Another file's import as the receiver** (`check_name.rs`, `term_import_prefix_for`). The
  prefixes of value imports are kept for the whole run, keyed by the member's owner. A service's
  `import gitbucket.core.model.Profile.currentDate` files `trait Profile` under the object path, and
  from then on a component's own `profile` -- and `dateColumnType`, the implicit a table class nested
  in the component takes from the self type -- was read as the object's. The test that a member is
  `this`'s asked only whether the current class *inherits* its owner; a self type is not
  inheritance, and a nested class inherits nothing of its component. It is now
  `SymbolTable::enclosing_class_reaching` (the class, a parent, or the self type, of the current
  class or any class enclosing it) -- the same walk `ident_prefix_class` and the macro wire's
  `this_qualifier_of` use. In gitbucket every `mapTo` prefix was
  `_root_.gitbucket.core.model.Profile.profile.api`; all 31 are `XComponent.this.profile.api` now,
  as nsc has them. With an implicit in `trait Profile` the same entry, a symbolic path, reached
  codegen as a receiver and the file was rejected ("unresolved ident").
* **A view from an object of an instance** (`implicits.rs`, `instance_object_import_prefix`).
  `import profile.obj._` where `obj` is an `object` inside `profile`'s class: the implicit was
  emitted as a bare name and loaded as the `obj` of a cast `this`. It now takes the path the scope's
  own binding was imported under.
* **An import root shadowed where it is used** (`check_name.rs`, `writable_import_prefix`). `def
  shadowed(profile: Int) = "s".shout` after `import profile.api._` still means `this`'s `profile`;
  the rewrite into `C.this.profile` only looked for `profile`'s owner among the *enclosing* classes,
  and a self type's member is not one of them.
### 7.26 `reify { … }` over the typed body

`reify` now walks the **typed** body and rebuilds every reference from the
symbol it resolved to -- nsc's own rule -- instead of classifying the parsed
body name by name (§7.15, §7.17). Locals and parameters bound outside the
body are *free terms* (`newFreeTerm` + `setInfo`), type parameters with no tag
in scope are *free types*, definitions inside the body (classes, objects,
defs, vals, closures, patterns) are reified by name, members of the enclosing
`object` through `mkThis`, and the tag materialiser builds its types through
the same type reifier (nested classes, singletons, aliases, `Predef.String`).
The design and what is still refused are in
[`docs/notes/reify-design.md`](notes/reify-design.md); the fixtures are
`tests/fixtures/reify2_*.scala` (`crates/cli/tests/reify2.rs`), each run
through real scalac 2.13.16 with identical output.

Shapes that earlier sections refused now compile, so their fixtures
changed: `rb_bad.scala` became `rb_free.scala`,
`rd_defs_bad.scala` became `rd_defs_typed.scala`, and `rf_bad.scala` and
`ex_notag_bad.scala` were removed (their shapes now run in `rf_more.scala`
and `ex_notag.scala`).

### 7.27 Quasiquotes in pattern position

`case q"..." =>` works. cats needs it:
`core/src/main/scala-2/cats/arrow/FunctionKMacros.scala` writes

```scala
case q"($param) => $trans[..$typeArgs]($arg)" if param.symbol == arg.symbol => …
```

(see [`docs/cats.md`](cats.md)).

#### What nsc does, and what we do instead

nsc expands the same compiler-internal macro with `nme.unapply` rather than
`nme.apply` and runs an `UnapplyReifier` over the same parsed body, which
builds a synthetic matcher —

```scala
{ new { def unapply(tree: $u.Tree) = tree match { case <reified> => Some(…); case _ => None } } }
  .unapply(<scrutinee>)
```

— whose inner pattern is written in the universe's own extractors.

`crates/typer/src/quasi_pattern.rs` builds that inner pattern **directly**,
with the hole patterns spliced in where they stand, and hands it back to
ordinary pattern typing. No synthetic class, no `Option` tupling: a hole of
rank 0 sits where a `Tree` is matched and a hole of rank 1 where a
`List[Tree]` is, which is exactly what the author's pattern wants to bind.

```text
q"($param) => $trans[..$typeArgs]($arg)"
  ⇒ u.Function(_root_.scala.List(param),
               u.Apply(u.internal.reificationSupport.SyntacticTypeApplied(trans, typeArgs),
                       _root_.scala.List(arg)))
```

Four things this had to get right, each checked against the real thing rather
than reasoned about:

1. **`SyntacticTypeApplied`, not `u.TypeApply`.** A quasiquote's written
   type-argument list matches a call with *or without* type arguments, because
   nsc's extractor is total — `unapply` returns `Some`, handing back `Nil` for
   a plain `Apply`. Run under real scalac on `q"f(1)"` and `q"f[Int](1)"`,
   `Apply(SyntacticTypeApplied(fun, targs), args)` and
   `q"$fun[..$targs](..$args)"` answer identically. A written type argument
   still discriminates: `q"$f[Int]($a)"` does not match `f(3)`, because
   `List(Ident(TypeName("Int")))` does not match `Nil`.
2. **Every name is qualified**: `u.Apply`, `_root_.scala.List`,
   `_root_.scala.Nil`. A bare `Apply` is resolved lexically, and the file that
   made this necessary is in package `cats`, which declares a `cats.Apply` of
   its own — a package member outranks a wildcard import (SLS 2), so the bare
   name bound to the type class and the pattern matched nothing at all. This
   is why the whole thing looked like a typer bug when it was hand-desugared:
   `case Apply(trans, List(arg))` in that package gives `arg: A` and
   `trans: Any`.
3. **Names are encoded.** Reflect `Name`s carry the encoded spelling, so
   `q"$a + $b"` matches a selection of `$plus`. Matching the raw `+` would have
   matched nothing, silently.
4. **A rank-0 hole standing for the whole body is typed by `$u.Tree`**
   (`case (x: u.Tree)`). nsc's generated matcher takes a `$u.Tree` parameter,
   so the hole is typed by it; spliced in bare it would have been an ordinary
   variable pattern binding the scrutinee at whatever type the scrutinee had,
   and `case q"$a"` would have matched an `Option[Int]`.

#### Shapes that work

Term quasiquotes (`q"..."`): literals; term identifiers and selections
(operators included, in their encoded spelling); applications, with or without
a written type-argument list; function literals whose parameters are spliced;
blocks (`SyntacticBlock`, which also answers for a single statement never
wrapped in a block); holes of rank 0 anywhere a tree stands and of rank 1
standing for a whole argument, type-argument, parameter or statement list.

#### Shapes that are refused, by name

Everything else is an error naming the shape, never a silent pass — a
quasiquote pattern that quietly matched the wrong trees would make a macro
implementation compile and then mis-expand. `tests/fixtures/
czero_quasipat_bad.scala` pins eight of them: a `..$` hole mixed with written
elements (`q"$f(..$as, y)"`), a parameter written out rather than spliced
(`q"(x: Int) => $r"`), a right-associative operator written infix
(`q"$a :: $b"`), `new`, `if`, the empty quasiquote, a `...$` hole, and
`tq"..."` / `pq"..."` / `cq"..."` in pattern position. Real scalac accepts all
but the `...$` one, so these are refusals and not wrong acceptances.

One shape is refused further up, in the quasiquote *front end*
(`crates/typer/src/quasiquote.rs`): a rank-1 hole standing for a list of
`case` clauses (`q"{ case ..$cases }"`, which is `pos/t8411`) cannot be
parsed, because the front end fills every hole with one placeholder *name* and
a case clause needs a `case … =>` shaped filler.

A hole whose pattern carries a type ascription (`q"${c: C}"`) is refused too,
unless `C` is a `Tree` subtype: nsc unlifts the matched tree through an
`Unliftable[C]`, which is not implemented (`crates/typer/src/quasi_pattern.rs`).

#### One documented difference from nsc

nsc's matcher **casts** its argument to `Tree` where this **tests** it, so for
a scrutinee statically wider than `Tree` the two differ: `x: Any` holding a
`String`, matched against `case q"$a"`, throws a `MatchError` from inside
nsc's synthetic `unapply` and falls through to the next case here. A type test
is what every nested hole already gets (`u.Apply(…)` cannot match a `String`
either), and no macro implementation has a scrutinee wider than `Tree`.

#### Validation

`tests/fixtures/czero_quasipat.scala` runs 22 trees through 11 quasiquote
patterns against `scala.reflect.runtime.universe` and prints what each hole
binds; `crates/cli/tests/czero.rs` compiles it with scala-rs *and* with real
scalac 2.13.16 and requires both runs' stdout to be byte-identical to the
recorded expectation, under `java -Xverify:all`. The matches and the
non-matches are both in there, which is what makes it evidence.

### 7.30 Whitebox macros

A whitebox macro's expansion type **replaces** the declared result type; a blackbox macro's does not.
That is the whole semantic difference, and it is what nsc's `macroExpandApply` expresses by wrapping
the expansion in `WhiteboxExpansion`: the expansion is typed against the *call site's* expected type
rather than against the declaration's, and the type it comes out with stands. A blackbox expansion is
typed against the declared result type and then ascribed with it (`Typed(expanded, TypeTree(innerPt))`),
so the call site never sees anything more precise than the declaration.

scala-rs now does the same, and a whitebox macro def is bound exactly like a blackbox one:

* `crates/typer/src/macros.rs` no longer refuses `!blackbox` at the binding, and
  `crates/typer/src/pickle_supply.rs` carries the pickle's `is_blackbox` into the
  `MacroBinding` instead of requiring it. The deprecated pre-2.11 spelling
  `scala.reflect.macros.Context` classifies as whitebox, which is what the library's own
  `@deprecated type Context = whitebox.Context` makes it.
* `Typer::expand_macro_application` (`crates/typer/src/expand.rs`) branches on `binding.blackbox`
  for both halves of the rule: which expected type the expansion is re-typed against, and whether
  the declared type is put back afterwards.
* The engine's `Context` proxy (`crates/typer/java/ScalaRsMacroEngine.java`) declares
  `scala.reflect.macros.whitebox.Context`, which *extends* the blackbox one, so one proxy serves
  both kinds. Declaring only the blackbox interface made every whitebox implementation an
  `IllegalArgumentException: argument type mismatch` out of `Method.invoke` -- not a diagnostic.
  The whitebox-only `openImplicits` is answered over the reverse channel (§7.20).

**A check the refusal had been hiding.** `neg/macro-bundle-ambiguous` was rejected only because its
bundle takes a whitebox `Context`. nsc's real reason is that `macro Macros.impl`, where
`class Macros(val c: Context)` declares an `impl` *and* `object Macros` declares one, "makes sense
both as a macro bundle method reference and a vanilla object method reference". Only candidates whose
**shape** fits the macro def count, which is exactly the line nsc's own three tests draw: with
`def foo: Unit`, `pos/macro-bundle-disambiguate-nonbundle` has only the object's fitting,
`pos/macro-bundle-disambiguate-bundle` only the bundle's, and `neg/macro-bundle-ambiguous` both.
`Typer::macro_bundle_companion` and `Typer::macro_clause_count` implement that. Macro bundles
themselves expand ([`refined.md`](refined.md);
`macro_bundle_metadata_and_expansion_interoperate_with_scalac` in
`crates/cli/tests/macrotransportbatch.rs`).
