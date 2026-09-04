# def Macro Design Notes

Design for handling Scala 2.13 **def macros** (`def f = macro impl`) in scala-rs.
The end goal is to compile slick without modification, and slick uses only two macros.

- `slick/lifted/ShapedValue.scala`
  `def mapTo[R <: Product with Serializable](implicit rCT: ClassTag[R]): MappedProjection[R] = macro ShapedValue.mapToImpl[R, U]`
- `slick/lifted/TableQuery.scala`
  `def apply[E <: AbstractTable[_]]: TableQuery[E] = macro TableQueryMacroImpl.apply[E]`

This document is the **deliverable of phase 0 (investigation and design)**. Even if the
implementation is unfinished, the design stays on record. Where something is infeasible or
unrealistic, it is stated as such.

## Table of contents

- 0. Summary (conclusions first)
- 1. How nsc handles def macros
  - 1.1 The definition side
  - 1.2 The call side (expansion)
  - 1.3 Execution
  - 1.4 Signature rules
- 2. Choosing an execution model
  - 2.1 Option A: an interpreter over our own AST
  - 2.2 Option B: a JVM bridge (adopted)
  - 2.3 Validation with a prototype (done)
  - 2.4 The honest cost of option B
  - 2.5 Intermediate options we rejected
- 3. The minimal subset of the reflect API we have to implement
  - 3.1 Context (the 72 methods we implement)
  - 3.2 The universe members `TableQueryMacroImpl.apply` touches
  - 3.3 The universe members `ShapedValue.mapToImpl` touches
- 4. Converting between our AST and reflect Trees
  - 4.1 Directions
  - 4.2 Wire format
  - 4.3 The limits of soundness (honestly)
- 5. What has to survive in the classfile (separate compilation)
- 6. A staged implementation plan
  - Phase 1 (the scope of this branch)
  - Phase 2: the engine and a minimal expansion
  - Phase 3: being able to compile macro implementations (the main event)
  - Phase 4: built-in (fast track) macros
  - Phase 5: slick's two macros
- 6.2 The biggest obstacle: quasiquotes and reify cannot be expanded through the JVM bridge
- 6.3 About whitebox
- 6.4 Risk list
- 7. Current state (what actually works on this branch)
  - 7.1 The quasiquote **front end** (`crates/typer/src/quasiquote.rs`)
  - 7.2 Holes plugged on the way to the reflect ABI
  - 7.3 Holes still open (what is needed next)
  - 7.4 Calling from the declaring class, and reification (the `agent/reify2` slice)
  - 7.5 What remains after this slice
  - 7.6 Macro implementation signatures and `import c.universe._` (the `agent/quasi` slice)
  - 7.7 The remaining reification shapes (the second `agent/reify2` slice)
  - 7.8 `Liftable`, `symbolOf` / `weakTypeOf`, and diagnosing `reify` (the `agent/liftable` slice)
  - 7.9 Quasiquoting definitions (the `agent/defquasi` slice)
  - 7.10 The three shapes that need fresh names (the `agent/freshname` slice)
  - 7.10 `TypeTag` / `WeakTypeTag` materialization (the `agent/typetag` slice)
  - 7.11 The engine — actually calling macro implementations (the `agent/engine` slice)
  - 7.12 `c.Expr[T](tree)` and `c.prefix` (the `agent/expr` slice)
  - 7.13 Stage D-1: `Function` / `ValDef` in expansion results (the `agent/staged` slice)
  - 7.14 Just before stage D-2: nested `object`s and `<val>.type` (the `agent/reifyd` slice)
  - 7.15 Expanding `reify { … }` (the `agent/reifybody` slice)
  - 7.16 `ShapedValue.mapToImpl` — three roots (the `agent/shaped` slice)

(The two `7.10` entries above are not a typo in this table of contents: the numbering is duplicated
in the document itself, and the numbers are left unchanged because other documents reference these
sections by number.)

---

## 0. Summary (conclusions first)

- The execution model we choose is the **JVM bridge**. We do not interpret macro implementations
  over our own AST.
- The rationale is that "`scala.reflect.macros.blackbox.Context` has only 72 abstract members, and
  every one of them is an ordinary JVM interface method that passes `scala.reflect.api.*` values
  around", plus the fact that we can plug **`scala.reflect.runtime.universe` (the complete
  implementation bundled in scala-reflect.jar)** straight into `c.universe`. The latter is
  guaranteed at the type level
  (`scala.reflect.internal.SymbolTable extends scala.reflect.macros.Universe`,
  `scala.reflect.runtime.JavaUniverse extends scala.reflect.internal.SymbolTable`).
- This design is **not armchair theory: it has been validated with a working prototype** (§2.3).
  We took a macro implementation compiled by scalac and invoked it through a Context built with
  Java's `java.lang.reflect.Proxy`, and confirmed that all three patterns — `reify`,
  **quasiquotes**, and `WeakTypeTag` — return exactly the reflect Trees we expect.
- However, **the distance to slick's two macros is very long**. The bottleneck is not "expanding a
  macro" but "**being able to compile the source of the macro implementation itself with
  scala-rs**" (§6.2). In particular, roughly 95% of the body of `mapToImpl` is quasiquotes.
- And quasiquotes and `reify` **cannot be expanded through the JVM bridge**. They have no
  implementation classfiles in scala-reflect.jar: they are **compiler-internal (fast track) macros
  of nsc** (demonstrated in §6.2). So quasiquotes and reify are the one part **scala-rs has to
  implement itself as a built-in**. That is the largest remaining piece of work.

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
- Both of slick's macros are **blackbox**, so whitebox is not needed for now.

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
  compiler-internal implementations. That is the crux of §6.2.

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

### 2.3 Validation with a prototype (done)

We wrote an approximately 180-line probe that does nothing but "build a `blackbox.Context` with
Java's `java.lang.reflect.Proxy` and return the runtime universe from `universe()`", and used it to
actually invoke macro implementations compiled by scalac. Thanks to JDK 17's
`InvocationHandler.invokeDefault`, the traits' default implementations (`weakTypeOf` and friends) run
for real. The code and reproduction steps are in
[`docs/macro-engine-prototype/`](macro-engine-prototype/).

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
and compiled. **Desugaring them from source is a separate problem**, and that is §6.2.)

Operational notes this probe turned up:

- The `TreeCreator` that `reify` generates looks symbols up with `mirror.staticModule("…")`.
  Therefore **the engine's JVM classpath must also carry the classes the compiled code refers to**.
  Loading only the macro implementation gives `ScalaReflectionException: object Helper not found`.
- `c.Expr[T](tree)` can be implemented by expanding to
  `universe.Expr(mirror, FixedMirrorTreeCreator(mirror, tree))(tag)`
  (`scala.reflect.internal.StdCreators$FixedMirrorTreeCreator`).

### 2.4 The honest cost of option B

- **Two new compile-time dependencies**: a JVM and `scala-reflect.jar`. Today scala-rs treats even
  scala-library.jar as optional (`--no-scala-library` gives a private runtime). Macros become "a
  feature that only works when the jar is present". When the jar is missing we **do not silently
  accept the program; we emit a diagnostic**.
- The engine has to be written in **Java**, not Rust (implementing Scala traits from Java). The build
  then needs `javac`. Whether to ship a prebuilt engine or run `javac` on first use is a separate
  decision.
- We have to fix an inter-process wire format (§4).

### 2.5 Intermediate options we rejected

- **Calling scala-compiler.jar directly**: delegating just macro expansion to nsc. That is no
  different from "calling scalac", and it would defeat the point of scala-rs being a Scala compiler.
  It would also be dishonest as a benchmark. Not taken.
- **Receiving the expansion result as a source string and re-reading it with the scala-rs parser**:
  partially adopted as a "wire format" in §4. But `showCode` drops symbols, so on its own it is
  unsound (it confuses distinct symbols that share a name). See the limits in §4.3.

---

## 3. The minimal subset of the reflect API we have to implement

**A caveat about how this was collected**: there is no slick source checkout on this machine.
What follows was measured by reading the **compiled slick 3.4.1** (`slick_2.13-3.4.1.jar`) from the
Coursier cache with `javap -c -p`. In 3.4.1, `mapToImpl` lives in `Shape.scala` and
`TableQueryMacroImpl` in `Query.scala`, so the file layout differs from the `scala-2/slick/lifted/`
arrangement of 3.5.x mentioned in the task statement. The API surface can be assumed nearly
identical, but **the source text itself is unconfirmed**. Settling it requires
`git clone https://github.com/slick/slick`.

The surface slick's two macros actually touch in the bytecode. Note that **the only thing we
implement on the engine side is the `Context`**; the members on the `universe` side are the real ones
from scala-reflect.jar and run as is. So the table below is a checklist for "is the engine broken?",
not a list of "things to rewrite in Rust".

### 3.1 Context (the 72 methods we implement)

Slick actually uses only the following. The rest **fail explicitly** with
`UnsupportedOperationException("… is not implemented")`.

| Member | Used by | Implementation approach |
| --- | --- | --- |
| `universe` | both | return the runtime universe |
| `mirror` | both (indirectly) | `universe.runtimeMirror(macroClassLoader)` |
| `Expr` / `Expr(tree)(tag)` | both | as in §2.3 |
| `WeakTypeTag` / `TypeTag` | both | return the identically named companions from `universe` |
| `weakTypeOf` / `typeOf` / `symbolOf` | both | the traits' default implementations run |
| `prefix` | `mapToImpl` | build an `Expr` from the call site's receiver Tree |
| `enclosingPosition` | `mapToImpl` | convert the call site's Span into a `Position` |
| `abort(pos, msg)` | `mapToImpl` | throw an exception, converted into an error diagnostic on the Rust side |
| `freshName` | via quasiquotes | a monotonically increasing counter |

`typecheck` / `inferImplicitValue` / `inferImplicitView` / `parse` / `eval` / `enclosingClass` and
the like are **not used by slick**. It is fine for these to blow up when called.
(`typecheck` and `inferImplicitValue` essentially mean "call the compiler proper back from the
engine"; implementing them would require reverse RPC from the engine to Rust. See the risks in §6.4.)

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
  arguments), so to begin with "Literal / Ident / Select / Apply / New / Function / Block" is enough.
- **Output (JVM → Rust)**: the expansion result Tree. Here we do have to read **everything**.

### 4.2 Wire format

The `showRaw` form (`Apply(Select(Ident(Helper), TermName("hello")), List(Literal(Constant(7))))`)
comes out directly, as the prototype confirmed, but **re-parsing it is a lot of work on the Rust side
and its escaping rules are murky**. Serializing to JSON on the engine side is more reliable.

```json
{"t":"Apply",
 "fun":{"t":"Select","qual":{"t":"Ident","name":"Helper","sym":"slick.lifted.TableQuery$"},
        "name":"hello"},
 "args":[{"t":"Literal","const":{"k":"Int","v":7}}]}
```

Every Tree node carries a `t`, and nodes with a resolved symbol also carry a `sym` (fully qualified
name). The Rust side prefers `sym` when resolving and falls back to name resolution otherwise.

### 4.3 The limits of soundness (honestly)

- The expansion result Tree points at **symbols of the runtime universe on the JVM side**. Those are
  distinct from the symbols in the Rust-side SymbolTable. Matching them by the fully qualified name
  in `sym` is the bridge, but **symbols with no fully qualified name** — local variables, type
  parameters, anonymous function parameters — can only be carried by name. This can break variable
  capture (hygiene).
  Since nsc's def macros are not hygienic either (the culture is to work around it with
  `freshName`), we can plausibly settle for "as unsound as the real thing".
- If a **Tree with an embedded Type**, such as `TypeTree(tpe)`, comes back, the Type has to be
  serialized the same way and turned back into a Rust-side `Type`. Both slick macros use this, so it
  is mandatory (`TableQueryMacroImpl` produces `TypeTree(e.tpe)`).
- Re-reading a `showCode` string with the scala-rs parser drops the `sym` above and is therefore
  **unsound in general**. Keep it to debug output.

---

## 5. What has to survive in the classfile (separate compilation)

Since macro defs are expanded from other compilation units, the `ScalaSignature` must record "this
method is a macro, and its implementation is X.y".

- nsc: bakes `@macroImpl(tree)` (the six fields of §1.1) into the pickle's `SYMANNOT` and sets the
  `MACRO` flag (`1L << 15`). The body of a macro def is `EmptyTree`, and **no JVM method is emitted**
  (which is why macros cannot be called from Java). To catch leaks, RefChecks has a
  `"macro has not been expanded"` check.
- scala-rs today: `crates/backend/src/pickle.rs` can write `SYMANNOT` (proven with `@deprecated` and
  others), but **deliberately does not pickle the `MACRO` flag** (see the comment at the top of that
  file). On the unpickler side, the `PickledMethod` read by `crates/typer/src/classpath.rs` recovers
  only name / param / ret / tparams.
- Work needed:
  1. Give `Symbol` a `macro_impl: Option<MacroBinding>` (done; `crates/typer/src/symbol.rs`).
  2. On the pickle side, write the `MACRO` flag and the implementation reference for macro defs.
     For nsc compatibility this means the `TREE` representation of `@macroImpl`; if we only need to
     talk to ourselves, a simpler encoding would do. Since there is already a compatibility test in
     which **scalac reads our classfiles** (`scalac_typechecks_against_our_classfiles_if_present`),
     aiming at the nsc-compatible shape is worth it.
  3. Recover it on the unpickler side.
- **Macro defs emit no method body** (`crates/backend/src/gen.rs`).

---

## 6. A staged implementation plan

### Phase 1 (the scope of this branch)

1. The parser accepts `= macro <ref>`. Introduce `TreeKind::MacroRhs { impl_ref }`. **Done**
2. Add `Symbol.macro_impl` / `MacroBinding`. **Done**
3. The typer recognizes macro defs and:
   - diagnoses an omitted return type,
   - resolves the implementation reference and records the binding,
   - **explicitly diagnoses** "cannot expand" at the call site (never silently accepts).
4. The backend emits no body for macro defs.
5. Fixtures (prefix `macro`) and `crates/cli/tests/macros.rs`.

### Phase 2: the engine and a minimal expansion

6. The Java macro engine (the 72 `Context` methods, JSON serialization).
7. Launch the engine from the Rust side, receive `Literal(Constant(42))`, and splice it into the call
   site. `M.f()` returns `42`.
8. But **phase 2 has a prerequisite**: being able to compile macro implementation sources with
   scala-rs. That is §6.2.

### Phase 3: being able to compile macro implementations (the main event)

9. A prelude for `scala.reflect.macros.blackbox.Context` / `scala.reflect.api.Universe`
   (`crates/typer/src/prelude_reflect.rs`).
   This needs **path-dependent types** such as `c.Expr[T]`. We have confirmed that today scala-rs
   brings in type members via `import c.universe._` but **not term members** (in a probe, `Tree`
   resolved but `mk` gave `not found: value mk`).
10. Code generation for these, equivalent to `library_abi`. `Literal(Constant(42))` becomes
    `c.universe().Literal().apply(c.universe().Constant().apply(box(42)))`.

### Phase 4: built-in (fast track) macros

11. A desugarer for `reify`. Needed by `TableQueryMacroImpl.apply`.
12. A desugarer for quasiquotes (§6.2). Needed by `ShapedValue.mapToImpl`. **The single largest item.**

### Phase 5: slick's two macros

13. Get `TableQueryMacroImpl.apply` through (requires 11).
14. Get `ShapedValue.mapToImpl` through (requires 12). We also need to confirm that case class field
    enumeration (`Type.decls.collect` / `Type.member`) works across the engine.

---

## 6.2 The biggest obstacle: quasiquotes and reify cannot be expanded through the JVM bridge

Running the expansion is solved by §2.3. **The real remaining difficulty is whether scala-rs can
compile the source of the macro implementation.** And within that there is one decisive fact.

### The facts

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

### Consequences

- **The JVM bridge (option B) cannot be used for these.** There is no implementation class to load.
- Therefore **scala-rs has to implement them itself as built-ins**. This is **not** something that
  comes for free once you have a macro expander.
- Our earlier assumption that "quasiquotes are whitebox macros, so the expander will handle them" was
  **wrong**. Correcting it here for the record.

### So what do we build?

Fortunately the shape of what has to be built is clear. All nsc's quasiquote macros do is
**"parse the interpolated string as Scala and desugar it into a sequence of
`internal.reificationSupport.Syntactic*` calls"** (the bytecode measurements in §3.3 back this up:
the body of `mapToImpl` desugars into 209 `Syntactic*` call sites).

So the work on the scala-rs side is:

1. **Parse the contents of `q"…"` as Scala, in a form that permits holes** (`$x` / `${…}`).
   scala-rs already has a Scala parser, so this is an extension.
2. Lower the parse result into an AST of `Syntactic*` calls
   (`SyntacticSelectTerm` / `SyntacticApplied` / `SyntacticValDef` / `SyntacticDefDef` /
   `SyntacticNew` / `SyntacticFunction` / `SyntacticBlock` / `FlagsRepr` / …).
   The list in §3.3 is the minimal set needed to get slick through.
3. Generate code for that AST against the scala-reflect ABI (the same machinery as item 10 of
   phase 3).

That gives us **a compiled `mapToImpl`**, and from there the engine already validated in §2.3 runs
it. The reason the quasiquote-based `qqImpl` worked in §2.3 is precisely that we verified the second
half of this path first.

`reify` likewise needs a built-in that "desugars the reified block into universe Tree construction
calls (generating a `TreeCreator` / `TypeCreator`)". Needed by `TableQueryMacroImpl`.

### An honest size estimate

- The quasiquote desugarer: **bigger than this phase**. Once you include dispatch on the type of the
  hole (Tree / Name / Type / List / name), the expansion of `..$` / `...$`, and the pattern side
  (`unapply`), it is a substantial amount. That said, slick uses only the `apply` side; it does not
  use pattern quasiquotes.
- `reify`: medium. `TableQueryMacroImpl`'s usage is a straightforward reify of a single expression.
- Both are "new components written in Rust"; the only existing asset we can reuse is the parser.

### Alternatives (not taken, but recorded)

It is technically possible to operate as follows: compile only slick's `ShapedValue.scala` with
scalac to have the classfile on hand, and let scala-rs handle only the expansion.
But that damages the meaning of the "scala-rs compiles slick" benchmark, so if we do it we must
**say so explicitly** and not count it as a benchmark result.

## 6.3 About whitebox

Slick's two macros are blackbox. Quasiquotes and reify do not require a whitebox expander either
(they are fast track). So **whitebox is not needed at all for now**. We implement only blackbox, and
when we find a whitebox macro def we diagnose and fail.

## 6.4 Risk list

| Risk | Impact | Mitigation |
| --- | --- | --- |
| Macros using `c.typecheck` / `inferImplicitValue` | Requires bidirectional RPC calling the Rust typer back from the engine | Slick does not use them. Diagnose and fail if called |
| Hygiene (§4.3) | The expansion result captures variables at the call site | nsc is non-hygienic too. Rely on `freshName` |
| Round-tripping Types | Without being able to return `TypeTree(tpe)`, `TableQueryMacroImpl` does not work | Serialize Types to JSON as well (mandatory work) |
| Engine process startup cost | Slow on large builds | Keep it resident and handle many expansions in one process |
| Dependency on scala-reflect.jar | Macros unusable in environments without the jar | Diagnose and fail. Document that the private runtime does not support them |
| Dependency on `javac` | One more build-environment requirement | Ship a prebuilt engine, or gate it behind a feature |
| Differences between the runtime universe and the compiler universe | Some macros change behavior | nsc's implementation classes **declare `c.universe` as `scala.tools.nsc.Global`** (the public API is `macros.Universe`). Macros written against the API work (demonstrated in §2.3), but macros that cast to `Global` do not. Diagnose and fail |
| Fast track macros (§6.2) | Macros using quasiquotes / reify **cannot be compiled at all** | We have to write the desugarers ourselves. Phase 4 |
| `MacroImplBinding` pickle compatibility | scalac would no longer be able to read our classfiles | Write the nsc-compatible shape, down to the `macroEngine` string |

---

## 7. Current state (what actually works on this branch)

- `= macro <ref>` **parses**. The old `unimplemented syntax: macros` is gone.
- The binding is recorded on the macro def's symbol.
- **Expansion still does not work.** A diagnostic is emitted at the call site. We never accept
  silently.
- The §2.3 prototype is in [`docs/macro-engine-prototype/`](macro-engine-prototype/). It does not run
  in CI (it needs scalac and scala-reflect.jar). How to run it, and what it lacks compared to a
  production version, is written in the README there. It will be formally absorbed as
  `crates/macro-engine/` in phase 2.

### 7.1 The quasiquote **front end** (`crates/typer/src/quasiquote.rs`)

Recognizing and diagnosing `q"…"` / `tq"…"` / `pq"…"` / `cq"…"` works. Previously we emitted the
**incorrect** diagnostic `value q is not a member of StringContext` (`q` is a member of
`Quasiquotes.Quasiquote`; what is missing is the expansion).

- The contents of the interpolated string are **reconstructed with the holes
  (`$x` / `${…}` / `..$xs` / `...$xss`) replaced by placeholder names, and actually parsed by the
  scala-rs parser**. Since `..` / `...` appear at the end of the preceding part, the rank is stripped
  from there.
- If it does not parse: `unimplemented syntax: quasiquote q"..." (reason)`.
- If it does parse, the remaining gap is reification, so we emit
  `macro expansion is not implemented: cannot expand quasiquote q"..."`.
- **We do not hijack user-defined `q` interpolators.** We first try to type it as an ordinary custom
  interpolator, and only report it as a quasiquote when that fails (the fixture `quasi.scala`
  verifies this all the way to run time).

**Measured on slick**: all 14 sites in `ShapedValue.mapToImpl` (`q` 12 / `tq` 1 / `pq` 1) are
recognized, and **not one `unimplemented syntax` is emitted**. That is, **the scala-rs parser can
parse the entire contents of every quasiquote slick uses**. What remains are items 2 and 3 of §6.2:
the reification that lowers the parse result into `Syntactic*` calls, and the code generation for it.

### 7.2 Holes plugged on the way to the reflect ABI

Being able to expand `q"…"` is pointless if scala-rs cannot typecheck what it lowers to (`c.universe`
/ the runtime universe). As groundwork for phase 3 we implemented the following. All of them are
general fixes, not reflect-specific ones.

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

### 7.3 Holes still open (what is needed next)

**A. Calling from the class that declares the target. Done (`agent/reify2`).** Item 1 of §7.4.

**B. Reification proper. Partly done (`agent/reify2`).** Item 2 of §7.4. The subset implemented, and
the shapes we still cannot lower, are listed in §7.4.

**C. Path-dependent types such as `c.Expr[T]`. Done (`agent/quasi`).** §7.6.

**D. The engine (phase 2).** Even with A through C done, *calling* slick's `mapToImpl` requires the
JVM bridge of §2.3. That part is already validated by the prototype and can come last.

### 7.4 Calling from the declaring class, and reification (the `agent/reify2` slice)

A and B of §7.3. **Code that builds Trees on `scala.reflect.runtime.universe` now actually runs**,
and on top of that some shapes of `q"…"` really get desugared.

#### 1. Calling from the declaring class (A, done)

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

#### 2. Reification (B, partial implementation)

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
| `f()` | `Nil` (`List()` cannot resolve `A`; see §7.5) |

**Shapes we cannot lower are always diagnosed** (the `unimplemented syntax: quasiquote q"..." (…)`
message names which shape it was). The holes as of this slice (**mostly filled in §7.7**): blocks,
function literals, `new`, `if`, `match`, type ascriptions, type applications, `this` / `super`,
definitions (`val` / `def` / `class`), mixing `..$` with ordinary arguments, and all of
`tq` / `pq` / `cq`.

Validation: `tests/fixtures/reify_qq.scala` is dual-run against the real scalac 2.13.16 and
**the output matches exactly** (`crates/cli/tests/reify.rs`). The failure cases are in
`tests/fixtures/reify_qq_bad.scala`.

### 7.5 What remains after this slice

1. **`tq` / `pq` / `cq`.** `mapToImpl` uses all three. `tq` needs roughly
   `SyntacticAppliedType` / `SyntacticSelectType` / `SyntacticTypeIdent` /
   `SyntacticEmptyTypeTree`, `pq` needs `Bind` / `UnApply`, and `cq` needs `CaseDef`.
2. **The remaining shapes of `q`.** In particular `SyntacticBlock` (multi-statement `q"""…"""`),
   `SyntacticNew`, `SyntacticFunction`, `SyntacticValDef` / `SyntacticDefDef`, and `Typed`
   (`(x: T)`). The occurrence counts in §3.3 are the priority order.
2. **Mixing `..$` with ordinary arguments** (`q"f(a, ..$xs)"`). The static type of the concatenation
   has to come out right on both sides.
3. **Inferring method type parameters from the expected type.** `List()` stays unresolved as
   `List[A]`, so we work around it with `Nil`. With this in place, writing the mixed concatenation
   also becomes easier.
4. **`Liftable`.** When the `x` in `$x` is not a `Tree` (`Int`, `String`, `Name`, `Symbol`,
   `WeakTypeTag`), nsc lifts it via an implicit `Liftable`. `mapToImpl` uses this for
   `$rTag` / `$rCT` / `${c.prefix}`. Today a non-`Tree` hole is a type error (we do not accept it
   silently).
5. **C (path-dependent types such as `c.Expr[T]`) and D (the engine) of §7.3.** C was finished in
   §7.6. D (the engine) remains.

### 7.6 Macro implementation signatures and `import c.universe._` (the `agent/quasi` slice)

C of §7.3. **If scala-reflect.jar is on the classpath, macro implementation sources now compile.**
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
  scalac compile it, and the classfiles emitted load and verify on the JVM. Expansion needs the
  engine (D), so we do not run it.
- `tests/fixtures/qq_ctx_bad.scala` — shapes that cannot be reified (type ascriptions, blocks, `tq`)
  are always diagnosed **by naming the shape**. Non-`Tree` holes are type errors too.
- The diagnostic for the empty `Context` without scala-reflect.jar is pinned as well.

**How this affects slick (important).** The `deps.cp` used by `tests/slick_measure.sh`
**does not contain scala-reflect.jar**. Slick itself depends on it (`scala-reflect` in `build.sbt`),
so without it even the real scalac cannot compile `ShapedValue.scala` / `TableQuery.scala`.
The numbers:

| classpath | errors | ShapedValue | TableQuery |
| --- | --- | --- | --- |
| Default (no scala-reflect) | 327 → **320** | 29 → 29 | 23 → 23 |
| Adding `-cp scala-reflect.jar` | 322 → **294** | 26 → **17** | 21 → **9** |

(Both before and after measured at the branch point `6c6fc7f`. Merely adding the jar moves 327 → 322
because several `scala.reflect.*` names other than `Context` start resolving.)

Changing the default classpath on our own would move other agents' baselines too, so `deps.cp` has
not been touched. **To reduce the 12 quasiquote errors, the measurement classpath first has to gain
scala-reflect.jar.**

On top of that, what remains:

1. **The remaining reification shapes.** Of the 17 errors left in `ShapedValue.scala` with
   scala-reflect.jar added, **11** are these, broken down as `Typed` (type ascription, 8),
   `SyntacticBlock` (1), `tq` (1), `pq` (1). Exactly items 1 and 2 of §7.5. Every one of them is now
   a diagnostic that names **which shape is missing**, rather than "cannot expand".
2. **The `Ident(TermName("x"))` / `New(TypeTree(…))` overloads.** `apply` insertion does not work
   against the overload set of `val Ident: IdentExtractor` and `def Ident(name: String): Ident`.
   The 2 errors in `TableQuery`.
3. **`symbolOf[T]` / `typeOf[T]`.** Members whose type parameter appears only in the implicit section
   are explicitly refused by `pin_undetermined_tparams` (a general restriction).
4. **Shadowing of wildcard imports.** `import c.universe._` should shadow the implicit
   `import scala._`, but `Symbol` still resolves to `scala.Symbol`.
5. **scalac cannot read our pickle.** Referring from the real scalac, with `macro`, to a macro
   implementation compiled by scala-rs gives
   `macro implementation has incompatible shape: found (c: Context, x: Tree): Tree`.
   The parameter sections have been collapsed into one and the path-dependent types are gone.
   This is the phase 2 work of §5.

### 7.7 The remaining reification shapes (the second `agent/reify2` slice)

Items 1 and 2 of §7.6. **`tq"…"` / `pq"…"` / `cq"…"` and the remaining shapes of `q"…"` can now be
lowered.** Every shape was read off the real scalac 2.13.16 with `-Ymacro-debug-lite` (which prints
the expansion nsc's own quasiquote macros emit), and `tests/fixtures/qr_forms.scala` compares against
the real scalac down to `showRaw` (run under `java -Xverify:all`; 56 lines match exactly).

#### Shapes that can now be lowered

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

#### Shapes we cannot lower are still diagnosed by name

We do not build shapes where the parser has discarded **the information itself**, so that whatever we
built would be "a tree nobody wrote". `tests/fixtures/qr_forms_bad.scala` / `reify_qq_bad.scala` /
`qq_ctx_bad.scala` each pin the corresponding diagnostic.

| Shape | Diagnostic | Reason |
| --- | --- | --- |
| `q"a :: b"` | a right-associative operator (`::`) is not reified yet | Parsing yields `b.::(a)`, indistinguishable from a written `b.::(a)`. nsc builds neither: it builds a **block** that binds the left-hand side to a fresh `val` |
| `q"if (a) b"` | an `if` without an `else` is not reified yet | The parser fills the `else` with `()`. nsc fills it with an empty block |
| `q"_.get"` | a `_` placeholder function literal is not reified yet | The parameter name the parser makes differs from nsc's `freshTermName` |
| `tq"=> T"` | a by-name type is not reified yet | nsc's own parser rejects it inside `tq` |
| `q"f(a, ..$xs)"` | a `..$` splice mixed with ordinary arguments | The static type of the concatenation has to come out right on both sides (item 2 of §7.5) |
| Definitions such as `q"class C"` | a class definition is not reified yet | `SyntacticClassDef` and friends were unimplemented (**added in §7.8/7.9**) |
| `q"{ lazy val a = 1 }"` | a modified `val` definition is not reified yet | The flag conversion for `Modifiers` was unimplemented (**added in §7.8/7.9**) |
| `q"{ $x }"` | (no diagnostic; a known difference) | The parser collapses `{ e }` into `e`, so a lone hole comes out as `x` where nsc has `SyntacticBlock(List(x))`. The meaning is the same, the tree is not |

#### General holes fixed along the way

Reification merely happened to demand these; none of them is reflect-specific.

| What was fixed | Where |
| --- | --- |
| **Inserting `apply` on an overload set.** `val Ident: IdentExtractor` and `def Ident(name: String): Ident` form one overload set under the same name, and `Ident(TermName("x"))` matches neither: it is `Ident.apply(...)`. `Bind` / `This` / `New` have the same shape. Per item 2 of §7.6, slick's `TableQuery` macro implementation is written entirely out of this | the `Type::Overload` branch of `Check::insert_apply_on_nullary` |
| **A term selection being eaten by a type member of the same name.** The reflect API puts both `type Modifiers` and `def Modifiers(flags: FlagSet)`. Since jar members are lazily loaded name by name, once **the type member goes in first** (completing `NoMods` brings it in) the name is no longer "not found", so the term overload was never read and `u.Modifiers(flags)` resolved to a `TypeMember` of `<notype>` (`value apply is not a member of <notype>`). The mirror image of `expose_unqualified_type` in §7.6 | `Check::type_select` |
| **The `count` of `invokeinterface` not being a slot count.** `long` / `double` arguments take two slots. `reificationSupport.FlagsRepr(8192L)` was giving `VerifyError: Inconsistent args count operand in invokeinterface` | `Assembler::invokeinterface` / `count_param_slots` |
| **No erasure-adapting `checkcast` on abstract type member arguments.** `type TermName >: Null <: TermNameApi with Name` erases to `Names$TermNameApi` and `Name` to `Names$NameApi`, and the JVM does not know how the two relate. nsc emits a `checkcast` here | `adapt_type_member_arg` in `gen.rs` |
| **`NoMods` is declared on `Universe`.** `scala.reflect.api.Universe` is an abstract class, and `JavaUniverse`'s inheritance from it exists only in the pickle. `u.NoMods` became `invokevirtual scala/reflect/api/Universe.NoMods()` and failed verification. Reification uses `u.Modifiers(rs.FlagsRepr(0L))`, which builds the same value (`Modifiers(flags)` is `Modifiers(flags, typeNames.EMPTY, Nil)`) | `Reifier::mods` |

#### How this affects slick

With `tests/slick_measure.sh` (with scala-reflect.jar), `errors=257 → 255`.
The number barely moves because **the same lines now fail for different reasons**; the
quasiquote-related breakdown is as follows.

| Diagnostic | before | after |
| --- | --- | --- |
| `unimplemented syntax: quasiquote …` (a missing shape) | 10 | **4** |
| `cannot expand quasiquote …` (no reify at all) | 1 | **0** |
| Total errors in `TableQuery.scala` | 11 | **6** |

The remaining 4 break down as 3 occurrences of `q"…_.get…"` (the `_` placeholder) and one `type`
definition inside a `q"""…"""`. The 8 type ascriptions in `ShapedValue.mapToImpl` **now go through as
far as shape is concerned**; what fails now is that `$uTag` / `$rTag` are `WeakTypeTag`s and not
`Tree`s (item 4 of §7.5, `Liftable`):

```
error: no matching overload for SyntacticFunctionTypeExtractor
       with arguments (List[TypeTags$WeakTypeTag[U]], TypeTags$WeakTypeTag[R])
```

So the next move for `mapToImpl` is **`Liftable`**, not more shapes.

#### What remains after this slice

1. **`Liftable`.** Lifting non-`Tree` holes (`WeakTypeTag` / `Name` / `Int` / `String` / `Symbol`)
   via implicits. Everything left in `ShapedValue` is this.
2. **The `_` placeholder and right-associative operators.** For both, nsc builds a block using
   `freshTermName`. If we build them, we have to build the same shape.
3. **Mixing `..$` with ordinary arguments**, and **inferring type parameters from the expected type**
   (§7.5).
4. **Quasiquoting definitions** (`SyntacticClassDef` / `SyntacticDefDef` / the flag conversion for
   `Modifiers`). The whole `q"""…"""` of `ShapedValue` needs this.
5. **`reify { … }` and `typeOf[T]` / `symbolOf[T]`.** `reify` is a fast track macro just like
   quasiquotes and needs our own implementation. Three of the six errors left in `TableQuery` are
   this.
6. **The engine (phase 2).** The JVM bridge for actually *calling* macros.

### 7.8 `Liftable`, `symbolOf` / `weakTypeOf`, and diagnosing `reify` (the `agent/liftable` slice)

Items 1 and 5 of the §7.7 list. **Non-`Tree` holes now lift**, so the
`q"($rModule.tupled) : ($uTag => $rTag)"` family in `ShapedValue.mapToImpl` no longer fails with
either "missing shape" or "the hole is not a `Tree`".

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

Item 3 of §7.6. `def symbolOf[T](implicit tag: WeakTypeTag[T]): TypeSymbol` mentions its type
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
- **Outside, the diagnostic becomes honest.** `u.typeOf[Int]` gives
  `no implicit: could not find implicit value of type TypeTags$TypeTag[Int]`.
  `TypeTag` materialization (the compiler-internal macro that reifies a type into a `TypeCreator`) is
  unimplemented, and that is the remaining obstacle for `c.typeOf[HList]`.

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

**Turning a whole expression into a tree is not implemented** (unlike quasiquotes, it requires
lowering an arbitrary expression into an anonymous `TreeCreator` class).

#### How this affects slick

With `tests/slick_measure.sh` (with scala-reflect.jar), `errors=237 → 228` and
`files_with_errors=60 → 60`. Breakdown:

| File | before | after |
| --- | --- | --- |
| `ShapedValue.scala` | 20 | **10** |
| `TableQuery.scala` | 6 | 7 |

`TableQuery.scala` gains one because `typeOf` changed from "not found" to "no implicit", which made
the second hole on the same line visible as well (the `Ident(sym: Symbol)` overload is not supplied).
The diagnostics are more accurate.

The 10 errors left in `ShapedValue.scala`:

| Diagnostic | Count |
| --- | --- |
| The `_` placeholder (`(_.get)`, the known shape from §7.7) | 3 |
| Holes that cannot be lifted (`<error>` / `AnyRef`; a cascade of the above) | 3 |
| `value collect is not a member of Scopes.MemberScope` | 1 |
| `no implicit: TypeTag[HList]` (materialization unimplemented) | 1 |
| Macro def signature checking (`must take blackbox.Context`) | 1 |
| A type mismatch in `Shape` (unrelated to quasiquotes) | 1 |

#### What remains after this slice

1. **`TypeTag` / `WeakTypeTag` materialization.** `c.typeOf[HList]` and `implicitly[TypeTag[T]]` need
   it. It is the compiler-internal macro that reifies a type into an anonymous `TypeCreator` class,
   and it works by the same mechanism as `reify { … }`.
2. **The body of `reify { … }`.** Turning a whole expression into a tree.
3. **The `_` placeholder and right-associative operators** (item 2 of §7.7).
4. **Quasiquoting definitions** (item 4 of §7.7). The whole `q"""…"""` of `ShapedValue`.
5. **Nested classes of the universe cannot be reached through a path.** `u.WeakTypeTag[T]` /
   `u.TypeTag.Int` give `value TypeTag is not a member of JavaUniverse` (`c.WeakTypeTag[T]` works
   because it is a type alias in `Aliases`).
6. **`c.universe.TermName` gives `stable identifier required`.** `c.universe` is a `val`, so it ought
   to be stable.
7. **The engine (phase 2).** The JVM bridge for actually *calling* macros.

### 7.9 Quasiquoting definitions (the `agent/defquasi` slice)

Item 4 of the §7.7 list. **`q"class C(...)"` / `q"case class C(...)"` / `q"trait T"` /
`q"object O { ... }"` / `q"def f(...) = ..."`, and modified definitions such as
`q"lazy val a = 1"`, can now be lowered.** Every shape was read off the real scalac 2.13.16 with
`-Ymacro-debug-lite`, and `tests/fixtures/dq_defs.scala` **compares 101 lines against the real scalac
down to `showRaw`** (run under `java -Xverify:all`; an exact match). The implementation is
`crates/typer/src/reify_defs.rs` (a `#[path]` child module of `reify.rs`; the split exists to avoid
touching the same file as `agent/liftable`, and the changes on the `reify.rs` side are only the `mod`
declaration, delegation in `stat`, two arms of `term`, and one hook in `new_spine`).

#### Shapes that can now be lowered

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
| `q"def f(x: Bar[_]) = x"` | a `_` type argument (an existential) … | nsc invents a name with `freshTypeName` and binds it in a block outside the call |

#### General holes fixed along the way

| What was fixed | Where |
| --- | --- |
| **`{ case class X(…); … }` was misread as a partial function.** A leading `case` in a block is a **modifier**, not the start of a clause, when what follows is `class` / `object`. A block containing a local `case class` was giving `expected pattern, found class` | `Parser::parse_block_expr` |

#### How this affects slick

With `tests/slick_measure.sh` (with scala-reflect.jar), `errors=237 → 237`.
**The number does not move.** The 15 error lines in `ShapedValue.mapToImpl` fail on `symbolOf`,
`Liftable` (`$uTag` / `$rTag` are `WeakTypeTag`s), and `_` placeholder function literals, and
definition shapes are none of those.
That said, the huge `q"""…"""` in the body (not a `case class` but three `val`s and a
`new … { ..$fpChildren; override def read … }` with a body) **now gets as far as `super` and a
`{..$xs}` right-hand side thanks to this slice, and its only remaining obstacle is the `_` type
argument (existential) in `ProductResultConverter[_, _, _, _]`**.
In other words the next move for `ShapedValue`'s `q"""…"""` is a "shape where nsc uses `fresh*Name`",
the same character as §7.7 — not definitions.

#### What remains after this slice (updating the §7.7 list)

1. **`Liftable`** (unchanged; the main cause in `ShapedValue`)
2. **The `_` placeholder / right-associative operators / `_` type arguments (existentials).** For all
   of these nsc builds a block using `freshTermName` / `freshTypeName`, so building the same shape
   means building the whole block that calls `rs.freshTypeName("_$")`.
   `ShapedValue`'s `q"""…"""` is stopped on this one alone
3. **Mixing `..$` with ordinary arguments**, and **inferring type parameters from the expected type**
   (§7.5)
4. **`q"{ type T = Int }"`** (`SyntacticTypeDef`)
5. **`reify { … }` and `typeOf[T]` / `symbolOf[T]`**
6. **The engine (phase 2)**

### 7.10 The three shapes that need fresh names (the `agent/freshname` slice)

Item 2 of the §7.9 list. **`_` placeholder function literals, `_` type arguments (existentials), and
right-associative operators can now be lowered.** These three differ from every earlier shape in one
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

#### Shapes that can now be lowered

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

#### How this affects slick

With `tests/slick_measure.sh` (with scala-reflect.jar), `errors=223 → 220` and
`files_with_errors=60 → 60`. Breakdown:

| File | before | after |
| --- | --- | --- |
| `ShapedValue.scala` | 10 | **7** |
| `TableQuery.scala` | 7 | 7 |

The three that disappeared are the `_` placeholders in
`(($rModule.unapply _) : $rTag => Option[$uTag]).andThen(_.get)` (lines 62 / 65 / 68).
`TableQuery.scala` fails on `reify { … }` and `TypeTag` materialization, unrelated to these three
shapes.

The huge `q"""…"""` in `ShapedValue.scala` (line 77) **now gets both
`ProductResultConverter[_, _, _, _]` (a type variable pattern inside a pattern) and
`TypeMappingResultConverter[…, _]` (an existential) through**, but what fails now is a cascade in
which the types of `$f` / `$g` come out as `AnyRef`, whose root cause is `rTag.tpe.decls.collect`
(`value collect is not a member of MemberScope`). No shape problems remain. The last line of
`fn2_fresh.scala` compares the same shape against the real scalac (with the holes swapped for ones
that can be lifted).

#### What remains after this slice (updating the §7.9 list)

1. **Collection operations on the reflect API such as `MemberScope#collect`** (the current root cause
   in `ShapedValue`)
2. **`TypeTag` / `WeakTypeTag` materialization** (`c.typeOf[HList]`, and `typeOf[Tag]` in
   `TableQuery`)
3. **The body of `reify { … }`** (turning a whole expression into a tree; the remainder of
   `TableQuery`)
4. **Mixing `..$` with ordinary arguments**, and **inferring type parameters from the expected type**
   (§7.5)
5. **`q"{ type T = Int }"`** (`SyntacticTypeDef`)
6. **The engine (phase 2)**

### 7.10 `TypeTag` / `WeakTypeTag` materialization (the `agent/typetag` slice)

Item 1 of the §7.8 list. **`typeOf[T]` / `weakTypeOf[T]` / `typeTag[T]` now actually work for
monomorphic types.** `c.typeOf[HList]` (in slick's `ShapedValue.mapToImpl`) and `typeOf[Tag]` in
`TableQuery` were stuck for want of this.

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

Three things were missing before `u.TypeTag.apply` could be called (this is exactly item 5 of the
§7.8 list).

| What was fixed | Where |
| --- | --- |
| **`TypeTags$TypeTag$` had no symbol.** The classfile of an object nested in a trait has no `ScalaSignature` of its own (the pickle is inside the enclosing `TypeTags`), so `install_classpath` skips it. As a result the descriptor `()Lscala/reflect/api/TypeTags$TypeTag$;` stayed an unresolvable `Type::Named` and we got `value apply is not a member of TypeTags$TypeTag$`. We now build the `ModuleClass` and insert `apply[T](Mirror, TypeCreator): TypeTag[T]` **by hand**. The erased descriptor is written out literally (if a method symbol's `jvm_name` starts with `(` it is taken as the descriptor — the same convention as pickle supply). The pickle's signature is `Mirror[TypeTags.this.type]`, and scala-rs cannot spell that singleton argument | `materialize::ensure_tag_module` |
| **The implicit parameter of `TypeTags#typeOf` was a `Type::Named`.** The pickle subset `install_classpath` reads holds member types by **simple name**, so nobody had installed the name `TypeTags$TypeTag` and it was unresolved. That was the true identity of `no implicit: could not find implicit value of type TypeTags$TypeTag[Foo]`, and erasure was about to write a descriptor out of that type | `materialize::resolve_named_tags` |
| **Sometimes the `TypeTags#TypeTag` accessor itself is absent.** If `TypeTags` is read as a classfile then `TypeTag()` appears in the method list, but when it comes via the pickle (nobody named it during the classpath scan) the module member is not among what `complete_named` installs, and the accessor is missing entirely. We write the descriptor and declare it here. Furthermore `TypeTags` is not a **direct** parent of `JavaUniverse` (it is a parent of `api.Universe`, and that link exists only in the pickle), so we first let `supply_from_pickle` walk the ancestors — otherwise **only the first `typeOf[T]` of a run** failed with "value TypeTag is not a member of JavaUniverse" | `materialize::ensure_tag_module` / `Check::materialize_tag` |
| **A resolved type cannot be spliced in as a type tree.** Neither the `T` of `TypeTag.apply[T]` nor the `api.Mirror` we cast to has a path reachable by name at the use site (`scala.reflect.api.Mirror` is not imported). We place the marker `Ident("$resolvedType")`, the counterpart of nsc's `TypeTree(tp)`, and `tree_to_type` returns its `ty` unchanged | `materialize::RESOLVED_TYPE` / `Check::tree_to_type` |

#### Shapes we can build, and shapes we refuse by name

`staticClass(<name>)` is a call that names **one class**, so what scala-rs builds is only
**class types with no type arguments**.

Buildable: the 9 primitive types / `Unit` / `String` / `Any` / `AnyVal` / `Nothing` / `Null` /
top-level classes and traits (`Foo`, `scala.math.BigInt`,
`slick.collection.heterogeneous.HList`).

Refused (pinned by `tests/fixtures/tt_tags_bad.scala`):

| Shape | Diagnostic | Reason |
| --- | --- | --- |
| `typeOf[List[Int]]` | a type constructor applied to type arguments | nsc builds a `TypeRef` from prefix, symbol and arguments |
| `typeOf[Nest.Inner]` | a class nested in a class or an object rather than a top-level one | `staticClass` only follows packages. nsc uses `selectType` |
| `typeOf[AnyRef]` | which is an alias rather than a class | An alias for `java.lang.Object`. `staticClass` fails at run time |
| `typeOf[T]` (a type parameter) | an abstract type with no tag in scope | nsc refuses it too (`No TypeTag available for T`). `WeakTypeTag` creates a free type, which is unimplemented |
| `typeOf[Main.type]` | a singleton type | |
| Structural types / function types / tuples / arrays | a structural type / whose type arguments would have to be reified too | |

The point is **never to silently build a different type**. A wrong tag is not a compile error; it just
arrives at the macro at run time as a "different `Type`", which makes it the hardest kind of defect to
find after the fact.

#### Validation

- `tests/fixtures/tt_tags.scala` — compiled and run with **both** scala-rs and the real scalac
  2.13.16, with the 30 lines of output matching exactly (`java -Xverify:all`).
  `tt_tags_materialises_type_tags` / `tt_tags_matches_real_scalac` in `crates/cli/tests/quasi.rs`.
- `tests/fixtures/tt_ctx.scala` — `c.typeOf[HL]` / `c.weakTypeOf[Rep]` inside a macro implementation
  (the shape of slick's `mapToImpl`). Both compilers accept it and the classfile loads and verifies
  on the JVM (expansion needs the engine).
- `tests/fixtures/tt_tags_bad.scala` — all 7 refused shapes are diagnosed by name.

#### How this affects slick

With `tests/slick_measure.sh` (with scala-reflect.jar), `errors=223 → 221` and
`files_with_errors=60 → 60`. Breakdown:

| File | before | after |
| --- | --- | --- |
| `ShapedValue.scala` | 10 | **9** |
| `TableQuery.scala` | 7 | **6** |

What disappeared in both cases is
`no implicit: could not find implicit value of type TypeTags$TypeTag[...]`:
`c.typeOf[slick.collection.heterogeneous.HList]` and `typeOf[Tag]` **actually go through** now.
Not one `TypeTag` implicit error remains in the log.

#### What remains after this slice

1. **Tags for types with type arguments.** `TypeTag[List[Int]]`. We would have to build nsc's
   `internal.reificationSupport.TypeRef` / `SingleType` / `selectType` into the creator's body.
   Nested classes (`selectType`) need the same toolkit.
2. **The type of a tag cannot be written by name.** `implicitly[TypeTag[Foo]]` fails not at
   materialization but where the **type name** `TypeTag` cannot be looked up (unqualified gives
   `not found: type TypeTag`, and through a path `u.TypeTag[Foo]` gives
   `type TypeTag is not a member of JavaUniverse`). This is still items 4 and 5 of the §7.8 list;
   `typeTag[Foo]` / `weakTypeTag[Foo]` demand the same implicit and do go through.
3. **`runtimeMirror(getClass.getClassLoader)`.** `java.lang.ClassLoader` has no symbol and
   `ensure_class` refuses pickle-less classes outside `scala.`, so the member is not supplied at all
   (`parameter cl has an unmappable type`).
4. **The body of `reify { … }`** (item 2 of the §7.8 list). Turning a whole expression into a
   `TreeCreator`. It rides on the same mechanism as materialization.
5. **The engine (phase 2).** The JVM bridge for actually *calling* macros.

### 7.11 The engine — actually calling macro implementations (the `agent/engine` slice)

**Phase 2** of §6. The §2.3 prototype became production code, and **a call to `def f = macro Impl.m`
is now really expanded, with the expanded program running**. It is dual-run against the real scalac
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
  that goes over a pipe with one request per line (the answer to "engine process startup cost" in the
  §6.4 risk table). It is killed from `Drop` when the `Typer` goes away.
- **The classpath is `binary_path` itself** (`-cp` plus `--scala-library`). This mirrors nsc, whose
  `-Ymacro-classpath` defaults to the compilation classpath, and it also satisfies the caveat found
  in §2.3 that "reify's `staticModule` also demands the classes being compiled".
- **The `Context` is a `java.lang.reflect.Proxy`** (as in the prototype). What we implemented is
  `universe` / `mirror` / `Expr` / `WeakTypeTag` / `TypeTag` / `TermName` / `TypeName` / `freshName` /
  `abort`, plus the traits' default implementations (`invokeDefault`). Everything else fails with
  `UnsupportedOperationException`, and the Rust side **puts that name in the diagnostic**.
- **Serialization is S-expressions** (not JSON). Both ends can write their own parser in 60 lines, and
  it rides the pipe one message per line. The information content is the same as the JSON proposal of
  §4.2.

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

Since it is blackbox, the expansion result is typechecked exactly once against **the declared return
type** as the expected type, and the type is put back to that declared type (nsc's
`Typed(expanded, TypeTree(innerPt))`).

**Everything that could not be expanded becomes a diagnostic, without exception.** The
`report_macro_calls` sweep is kept exactly as in phase 1; the expander merely records the **reason**
for each failure per span and hangs it there:

```
error: macro expansion is not implemented: cannot expand nameOf
       (implementation EgImpl$.nameOfImpl): scala-rs cannot build a type tag for
       `List[Int]`, a type constructor applied to type arguments. See docs/macros.md.
```

#### Two-pass compilation is by design

nsc decrees that "a macro implementation must have been compiled **before the run in which the
expansion happens**" (§1.3). scala-rs is the same: if the implementation is not on the macro
classpath, the engine returns `ClassNotFoundException`, which becomes the reason
`is not on the macro classpath (nsc requires the implementation to have been compiled by an earlier
run)` (pinned by `tests/fixtures/eg_samerun_bad.scala`).
The macro **def** side may live in the current run (that is slick's shape too).

#### Shapes that now work

| Shape | Example | Notes |
| --- | --- | --- |
| No arguments | `def const(): Int = macro EgImpl.constImpl` | The expansion is `Literal(Constant(42))` |
| A `c.Expr[T]` argument | `def plus1(x: Int): Int` | The call site's tree is wrapped in an `Expr` and passed |
| A raw `c.Tree` argument | `def twice(x: Int): Int` | The 2.11-and-later shape. This is what slick's `mapToImpl` uses |
| `c.WeakTypeTag[T]` | `def nameOf[T]: String = macro EgImpl.nameOfImpl[T]` | Type arguments only when **explicit** |
| Expansion result trees | `Literal` / `Ident` / `Select` / `Apply` / `TypeApply` / `Block` / `If` / `Typed` / `This` / `EmptyTree` / `TypeTree` | Anything else is refused by name |
| Static symbols | The `Ident(EgHelper)` of an expansion | If `isStatic`, expand to the fully qualified path and resolve at the call site |

#### Two general holes plugged on the way (both cases of "nobody had ever run this")

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

#### What remains after this slice

1. **`c.Expr[T](tree)` does not compile under scala-rs.** It does not resolve to the `Context.Expr`
   overload (`def Expr[T: WeakTypeTag](tree: Tree): Expr[T]`) but hits `universe.Expr.apply` instead.
   That is why every implementation in the fixtures returns a `c.Tree`. **Slick's
   `TableQueryMacroImpl` returns a `c.Expr`**, so we will need this.
2. **Inferred type arguments do not become tags.** We build a tag only when `M.f[T]` is written
   explicitly. Type arguments inferred at the call site are not left in the tree by the typer, so for
   now we refuse them by name.
3. **Argument trees can only carry "the syntax that was written".** Rather than passing typed trees
   as they are (§4.3), we pass `Literal` / `Ident` / `Select` / `Apply` / `This` as syntax and
   re-typecheck at the call site. Blocks, function literals, `new` and so on are refused by name.
   Slick's `mapToImpl` looks at `c.prefix`, so this, together with implementing `prefix`
   (unimplemented; `UnsupportedOperationException`), is the next move.
4. **`c.prefix` / `c.enclosingPosition` / `c.typecheck` / `c.inferImplicitValue`.**
   `prefix` is the receiver tree at the call site and `enclosingPosition` is a span conversion, both
   doable; `typecheck` / `inferImplicitValue` need reverse RPC from the engine to Rust (§6.4).
   What slick uses goes as far as `prefix` / `enclosingPosition` / `abort`, and `abort` is already
   implemented.
5. **A `TypeTree` in an expansion result may only be a class with no type arguments.** A tree with
   `List[Int]` embedded is refused.
6. **whitebox.** Still unimplemented (§6.3).
7. **The `MACRO` flag and the `@macroImpl` pickle (§5).** A macro def still cannot be expanded from
   *another run*. Only the shape "macro def in the current run, implementation from a previous run"
   works today. Slick puts the def and the implementation in one file, so this shape suffices.

#### How this affects slick

With `tests/slick_measure.sh`, `errors=203 → 203` and `files_with_errors=60 → 60`;
`tests/slick_subset.sh` stays at `204/204`. **The numbers do not move.**
The call sites of slick's `TableQuery.apply` / `ShapedValue.mapTo` are shapes that "have the
implementation in the same run", which nsc cannot expand either, so the engine only starts to matter
**once slick can first be compiled to classfiles**.
What this slice moves is not the "compile the implementation" side that §7.1 through §7.10 have been
building up, but the "call the implementation" side beyond it; it will start to matter for slick once
item 1 (`c.Expr`) and items 3 and 4 (`c.prefix`) land.

### 7.12 `c.Expr[T](tree)` and `c.prefix` (the `agent/expr` slice)

Item 1 (`c.Expr`) and part of item 4 (`c.prefix`) of the §7.11 list. Together with these,
**we can now assemble the `WeakTypeTag[F[E]]` that `c.Expr[F[E]]` demands**. With all three in place,
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

Shapes we cannot build are refused by name as before. Because the synthesis **recurses**,
`List[Nest.Inner]`, whose argument cannot be built, names the argument:
"`Inner`, a class nested in a class or an object". Tuples and function types (which would need
expansion to `scala.TupleN` / `scala.FunctionN`) and type parameters with no tag (nsc erects a free
type symbol; scala-rs does not) are still refused. `tests/fixtures/tt_tags_bad.scala` pins this.

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
- `tests/fixtures/tt_tags.scala` — materialisation outside a macro. We added
  `List[Int]` / `Option[Foo]` / `List[List[Int]]` and pinned that even the string of `tag.tpe` matches
  the real scalac (previously these were refused by name).
- `tests/fixtures/ex_notag_bad.scala` — tags that cannot be synthesized.
- `tests/fixtures/ex_gaps_bad.scala` — the two kinds of receiver we cannot carry.
  The real scalac accepts both, so these fixtures pin holes on the scala-rs side.

#### What remains after this slice

1. **We cannot build a `This` for `c.prefix`.** For a macro called without writing a receiver, nsc's
   prefix is `This(<the enclosing class>)`. `ex_gaps_bad.scala` pins this by name.
2. **Argument and receiver trees are still "the syntax that was written"** (item 3 of the §7.11 list).
   `new`, blocks and function literals cannot be carried. "Passing typed trees as they are" from §4.3
   is unimplemented; slick's `mapToImpl` looks only at the **tree** of `c.prefix`, so that much
   suffices, but an expression like `ShapedValue(...)` written as the receiver would not get through.
3. **We cannot build `Function` / `ValDef` / `Modifiers` in expansion results.**
   Slick's `TableQueryMacroImpl` passes `Function(List(ValDef(…)), …)` to `TableQuery.apply[E](cons)`,
   so **this is required to make real slick work**. Today it is refused by name.
4. **`reify`.** The last line of `TableQueryMacroImpl` is `reify { … }`, and being a fast track macro
   it cannot be expanded through the JVM bridge (§6.2). Compiling the implementation with scala-rs
   requires our own reify (the diagnostic is in §7.8).
5. Inferred type arguments not becoming tags (item 2 of the §7.11 list), type arguments not embeddable
   in a `TypeTree` (item 5 there), whitebox (item 6) and the `@macroImpl` pickle (item 7) are all
   unchanged.

#### How this affects slick

`tests/slick_measure.sh` gives `errors=177 → 177` and `files_with_errors=57 → 57`.
`tests/slick_subset.sh` stays at `38 files / 204 classes / verified=204 failed=0`.
**The numbers do not move.** As written in §7.11, slick's two macros are in the shape "def and
implementation in the same run", which nsc cannot expand either, and stage D (the experiment of
compiling slick in two stages) needs items 3 and 4 above.
What this slice moves is only as far as "we can write and expand a macro of **the same shape** as
slick's two macros".

### 7.13 Stage D-1: `Function` / `ValDef` in expansion results (the `agent/staged` slice)

Item 3 of the §7.12 list. **We can now build `Function` and `ValDef` in an expansion result**, so the
tree slick's `TableQueryMacroImpl.apply` assembles —
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
| **Tags for tuples, function types and arrays could not be built** (the known remainder from §7.12) | `Check::tag_body` | Name `scala.TupleN` / `scala.FunctionN` / `scala.Array` explicitly and put them on the `appliedType` synthesis of §7.12. Slick's `c.Expr[Tag => E]` demands this. `tt_tags.scala` pins that even `toString` matches the real scalac |

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

#### 4. What `reify` still lacks (findings for D-2)

Stage D-2 (our own `reify`) is **not implemented in this slice**. The design is settled, and
**we have confirmed that the tree we would need to build is accepted by the real scalac 2.13.16**, but
three holes remain on the scala-rs side before it.

The shape `reify { … }` should expand into (the same as nsc's `-Xprint:typer`):

```scala
{
  final class $treecreator1 extends scala.reflect.api.TreeCreator {
    def apply[U <: scala.reflect.api.Universe with Singleton](
        m: scala.reflect.api.Mirror[U]): U#Tree = {
      val u = m.universe
      u.internal.reificationSupport.SyntacticApplied(…)   // ← the reifier of §7.1
    }
  }
  c.universe.Expr.apply[T](
    c.universe.rootMirror.asInstanceOf[scala.reflect.api.Mirror[c.universe.type]],
    new $treecreator1())
}
```

**This shape is accepted by the real scalac** (including calling
`u.internal.reificationSupport.Syntactic*` through the path-dependent `U`). That is, if we hand the
reifier in `crates/typer/src/reify.rs` `m.universe` as its universe, the body can be reused as is —
and the `TypeCreator` synthesis in `crates/typer/src/materialize.rs` is the template for the
`TreeCreator` version.

Three holes remain unplugged on the scala-rs side, all of them problems that come before `reify`:

| Hole | Symptom |
| --- | --- |
| **Nested objects** of the universe cannot be reached through a path or through a wildcard import | `c.universe.Expr` gives `value Expr is not a member of Universe`, and `Expr` under `import c.universe._` gives `not found: value Expr`. `Exprs.Expr` is an `object` inside a trait, which `PickleSupply` does not supply (the same hole as item 5 of the §7.8 list) |
| `c.universe` cannot be written in a type as a **stable identifier** | `Mirror[c.universe.type]` gives `stable identifier required, but c.universe found` (item 6 of the §7.8 list). `c.universe` is a `val` and so ought to be stable. The synthesis side can avoid it by embedding the type directly with `RESOLVED_TYPE`, but the hole itself remains |
| **Hygiene** of the reify body | nsc's reify builds *typed* trees, so `TableQuery` resolves to `staticModule("slick.lifted.TableQuery")`. The reifier of §7.1 turns the written name into a `SyntacticTermIdent` as is, so it gets resolved in the scope of the expansion site. The design is to rewrite static symbols into fully qualified paths with `_root_.` and to **refuse everything else (locals, parameters) by name**, but it is unimplemented |

So `reify { … }` still gives the §7.8 diagnostic.

#### What remains after this slice

1. **An expansion's type argument has to be "a class from a previous run".**
   Since tags are built with `staticClass(<fully qualified name>)`, the engine's mirror can resolve
   **only classes on the macro classpath**. A row class defined in the *same run*, as in
   `TableQuery[Coffees]`, cannot be passed yet (pinned by `sd_gaps_bad.scala`). nsc uses the
   compiler's own universe and has no such restriction. **Getting real slick's usage side** through
   requires this.
2. **`reify`** (item 4 above), **`This` for `c.prefix`** (item 1 of the §7.12 list) and
   **passing typed trees as they are** (item 2 there) are unchanged.
3. **Overload selection for `TableQuery.apply[E](cons.splice)`.**
   `TableQuery.apply` has two forms, "one argument" and "no arguments (the macro)", and scala-rs picks
   the latter and then tries to apply `(cons.splice)` to the result, giving
   `value apply is not a member of TableQuery[E]`. nsc picks the former.
   One of the three things needed to get the real `TableQuery.scala` through (the other two are
   `reify`, and — unrelated to macros — the self name `base` of `new BaseTag { base => … }` not being
   resolvable).

#### How this affects slick

`tests/slick_measure.sh` gives `errors=155 → 154` and `files_with_errors=52 → 52`.
`tests/slick_subset.sh` stays at `38 files / 204 classes / verified=204 failed=0`.
The one that went away is `c.Expr[Tag => E]` (a function-type tag) in `TableQuery.scala`.
The rest is as in §7.12: slick's two macros are in the "def and implementation in the same run" shape,
and stage D-3 needs `reify`.

### 7.14 Just before stage D-2: nested `object`s and `<val>.type` (the `agent/reifyd` slice)

Of the three holes named in §7.13.4, **1 and 2 are now plugged**. Neither is `reify`-specific; both
are general features that also help code unrelated to macros. Item 3 (hygiene of the reify body) and
the expansion of `reify` itself are **still unimplemented in this slice**, and the diagnostic is
still the one from §7.8.

#### 1. `object`s inside a trait were not being supplied (item 5 of the §7.8 list)

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

#### 2. `c.universe` could not be written as a stable identifier in a type (item 6 of the §7.8 list)

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
in `rd_use.scala` really are expanded by the engine and print `42 / 42 / true`. What remains is only
"building this tree **automatically** from `reify { … }`".

#### What remains after this slice

1. **The expansion of `reify { … }` itself** (hole 3 of §7.13.4). The materials for the tree are all
   there; what remains is the synthesis on the check.rs side and **hygiene**. nsc's expansion shape
   (measured with `-Xprint:typer`) is

   ```scala
   { val $u: c.universe.type = c.universe
     val $m: $u.Mirror = c.universe.rootMirror
     $u.Expr.apply[T]($m, new $treecreator1())($u.TypeTag.apply[T]($m, new $typecreator2())) }
   ```

   with the creator's body being the reifier of §7.1 placed under `val $u = $m$untyped.universe`.
   For hygiene, static symbols are lowered to
   `$u.internal.reificationSupport.mkIdent($m.staticModule("RdHelper"))` and `splice` to
   `x.in[$u.type]($m).tree` — **both confirmed to work, written by hand, in `rd_impl.scala`**.
   The design is to refuse locals and parameters by name, and that is unimplemented.
   The synthesis side needs to know whether each identifier is a static symbol, so the natural
   approach is to resolve the body first in the same "type a clone speculatively and roll back" shape
   as `Check::hole_lifts`.
2. **Nested *classes* inside a trait** (writing `u.Liftable[Int]` as a **type**) still give
   `not found: type Liftable`. What we added this time is only the term side.
3. **The upper bound of `u.Mirror` cannot be read.** `Mirrors#Mirror` is
   `type Mirror >: Null <: api.Mirror[self.type]`, and `conv_upper_bound` drops this bound, so the
   `mm` of `x.in[u.type](mm)` has to be cast to `scala.reflect.api.Mirror[u.type]` rather than
   `u.Mirror` before being passed (nsc writes the former). See the comment in `rd_impl.scala`.
4. Items 1 and 3 of the §7.13 list (the expansion's type argument, and overload selection for
   `TableQuery.apply`) are unchanged.

#### How this affects slick

`tests/slick_measure.sh` gives `errors=134 → 134` and `files_with_errors=48 → 48`.
`tests/slick_subset.sh` stays at `38 files / 204 classes / verified=204 failed=0`. Slick's two macros
are stuck at the point where `reify` is required, and these two items only got things through the
stage before that, so the numbers do not move.

### 7.15 Expanding `reify { … }` (the `agent/reifybody` slice)

The tree that §7.14 got working "end to end when written by hand" is now built by the compiler.
`crates/typer/src/reify_expand.rs` builds exactly the nsc expansion shape written in item 1 of §7.14:

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

#### The body — hygiene

Lowering uses the same `Reifier` from `crates/typer/src/reify.rs` as quasiquotes, but runs in a
"reify mode" carrying a `ReifyCtx`. There are only three differences, and they all come down to
**resolving by symbol rather than by name**.

| Shape | Tree built |
| --- | --- |
| A static `object` | `$u.internal.reificationSupport.mkIdent($m.staticModule("<full name>"))` |
| `x.splice` | `x.in[$u.type]($m).tree` |
| Type arguments | `$u.internal.reificationSupport.mkTypeTree(<type>)` |
| Any other identifier, block, function literal, `this`, or type ascription | **a diagnostic** (`cannot expand reify { ... }: …`) |

That last line is the crux. nsc turns locals and parameters into *free terms*
(`newFreeTerm` + `mkIdent`) and carries them through the expansion, but scala-rs cannot build that.
Building them as bare names would **compile and run**, pointing at whatever happens to have the same
name at the expansion site — precisely the bug reification exists to prevent. So we refuse.

**Type arguments** are likewise not built by name. `f[E]` means "which `E` the macro implementation was
instantiated at", so building a `TypeTree` from the written name would give the same uncatchable bug.
The contents are made from the same materials as building a `TypeTag` (`crate::materialize::TagBody`),
and `Reifier::rebuild_type` writes them out against the creator's **cast** mirror `$m`
(`Mirror[$u.type]`):

| `TagBody` | Tree |
| --- | --- |
| `StaticClass(n)` | `$m.staticClass(n).asType.toTypeConstructor` |
| `Applied { c, args }` | `$u.appliedType($m.staticClass(c), List(<args>))` |
| `FromTag(tag)` | `tag.in[$u.type]($m).tpe` |

The materialiser's own creator can select directly on the parameter because its result erases to
`Types$TypeApi` and nothing more is stacked on it, whereas here the result is passed to `mkTypeTree`
and so must be a `$u.Type`. That last `FromTag` is what slick's
`reify { TableQuery.apply[E](cons.splice) }` requires.
A type we cannot build (an abstract type with no tag in scope, say) becomes a `ReifyRef::TypeGap` and
is diagnosed with the tag builder's own explanation attached.

Types **other than** type arguments (the right-hand side of a type ascription such as `(3: Int)`, for
example) are still an `Err`. We have no counterpart to nsc's `reifyType`.

#### How identifiers are classified

`Check::reify_refs` walks the body and, for each `Ident` / `Select`, **types a clone speculatively and
rolls back** (the same shape as `hole_lifts`). If the result is a `Type::ModuleRef` whose module
class's JVM name is reachable through packages alone (no `$` in the simple name) it is a static
`object`; if it is a `.splice` on an `Expr[T]` it is a splice; anything else is left unclassified,
i.e. the `Reifier` refuses it by name. Lookups are keyed by `NodeId`, which guarantees that the
classification and the lowering are looking at the same node.

Type arguments are turned into a `Type` by `tree_to_type` and handed to `Check::tag_body`
(`Tag::Weak`; since `TypeTag <: WeakTypeTag`, either kind of tag is found).

The `T` of `Expr.apply[T]` is obtained by speculatively typing the whole body exactly once
(a `Type::Constant` is widened with `lit_underlying`). The `WeakTypeTag[T]` of the implicit clause is
filled by the materialiser of §7.10, but that looks for the universe in an `import <universe>._`, so
for `c.universe.reify { … }` we push that universe as an import prefix **only while the expansion is
being typed** (we do not leave it pushed).

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
`rb_bad.scala` pins that the 5 refused shapes are diagnosed by name (the real scalac accepts all 5, so
this is a confession of what is unimplemented).

Slick goes from `errors=115 → 113` and `files_with_errors=41 → 41`.
`reify { TableQuery.apply[E](cons.splice) }` at `TableQuery.scala:50` **can now be expanded**, and the
two errors `cannot expand reify` and the `cannot expand apply` it dragged along with it are gone.
`crates/backend/` was not touched, so `slick_subset.sh` was not run.

#### What remains

1. The `value apply is not a member of TableQuery[E]` remaining on the same line is an item from the
   §7.13 list (overload selection for `TableQuery.apply`) and is a separate matter from reify.
2. Type arguments **inferred** at the call site still do not reach the macro (item 1 of the §7.13
   list). That is why `rb_use.scala` writes `RbUse.idOf[Int](5)` out explicitly.
3. *Free terms* for locals and parameters, blocks, function literals, `this`, and types other than
   type arguments.
4. The remainder from §7.14 (writing a nested *class* inside a trait as a type) is unchanged.

### 7.16 `ShapedValue.mapToImpl` — three roots (the `agent/shaped` slice)

We took `slick.lifted.ShapedValue` — of which §3.3 said "the body is almost entirely quasiquotes" —
**from 5 errors to 0**. Two of the 5 were quasiquote diagnostics about holes of type `<error>`, a
cascade of the three before them.

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

Slick goes from `errors=99 → 94` and `files_with_errors=39 → 38`. `ShapedValue.scala` goes
**5 → 0**. `crates/backend/` was not touched, so `slick_subset.sh` was not run.

#### What remains

1. **scala-rs's own `ScalaSignature` does not record case accessors.**
   A macro reads the members of a `WeakTypeTag` through the runtime mirror, so a case class compiled
   by scala-rs appears to have empty `decls`. Applying `mapTo[R]` to an `R` built by scala-rs silently
   produces an expansion with zero fields. That is why the fixtures enumerate library types
   (`Deadline` / `BigDecimal`).
2. **A type pattern against an abstract type member becomes `instanceof java/lang/Object`.**
   `erase_ty` lowers abstract type members to `Object` (whereas type parameters are lowered to their
   bound). A `case s: TermSymbol` test therefore passes everything through, so expanding `mapToImpl`
   for a type whose `decls` contain something that is not a `TermSymbol` gives an
   `IncompatibleClassChangeError` at run time. Fixing it means emitting the type pattern's `instanceof`
   with the bound's erasure, which reaches into codegen.
3. **A macro def read back from a scala-rs classfile is no longer a macro def.**
   `macro_impl` is not written to the pickle, so calling `mapTo` from another run compiles as an
   ordinary method call and gives a `NoSuchMethodError` at run time (with no diagnostic).
4. `_root_.scala.List` / `_root_.scala.Vector` give
   `no matching overload for <overload List$ | List$>`. Two copies of the same companion are in the
   scope of package `scala` (lexical `scala.List` avoids this by a different route).
5. **Expanding** `mapToImpl` needs, in addition to 1 through 3 above, anonymous classes in the
   expansion result (`expand.rs` has no `ClassDef` branch). This is not needed to compile slick itself.
