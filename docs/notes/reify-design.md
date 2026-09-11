# `reify { … }` over the typed body, and the type reifier (the `agent/reify` slice)

This note is the design of scala-rs's `reify`, the measured walls it was
built against, and what is left in order. It supersedes the reify parts of
`docs/macros.md` §7.15 and §7.17 (which are kept as history) and the
`agent/reifybody` / `reify` widening sections of
`docs/notes/macro-reflect-and-reify.md`.

## 1. The problem, measured

The scala/scala corpus (`/private/tmp/scala-rs-gate-908c5354-20260912/corpus.tsv`)
had about 130 `run` tests and a dozen `pos` tests stopping at reflection:
`cannot expand reify { ... }: X is a local, a parameter, ...` (41), `a class
definition is not reified yet` (33), `a function literal is not reified yet`
(12), `a type argument cannot be rebuilt` (11), `materialisation is not
implemented: cannot build a TypeTag for ...` (nested classes, singletons,
`AnyRef`, `_`, abstract types: ~30), `whitebox macros are not implemented`
(22), `not found: extractor Apply` (10), and more.

The old reifier (`docs/macros.md` §7.15) walked the *parsed* body and
classified each identifier by typing it on its own, speculatively; anything
it could not classify as a static `object` or a member of one it refused by
name, and it refused every definition, closure, `new`, `match`, `while`,
ascription and `this`.

## 2. Design: nsc's own rules over the typed tree

nsc's `scala.reflect.reify` (`Reifier`, `GenTrees`, `GenSymbols`, `GenTypes`,
`Reshape`) runs *after* the typer and rebuilds every reference from the
symbol it resolved to. scala-rs now does the same: `Check::try_expand_reify`
(`crates/typer/src/check_expr.rs`) types the body once on a clone, gathers
what only the typer can answer (`ReifyFacts`), and hands the **typed clone**
to `crate::reify::Reifier` in reify mode, whose reify-specific half is
`crates/typer/src/reify_tree.rs`. The quasiquote lowering (`reify.rs`,
`reify_defs.rs`) is reused for the syntactic shapes; the rules that differ
are exactly nsc's:

| reference | built as | nsc |
|---|---|---|
| a definition inside the body, and references to it | by name: `ValDef(...)`, `Ident(TermName("x"))` | `isLocalToReifee` → `reifyProduct` |
| a static `object` / a member of one | `mkIdent($m.staticModule("O"))`, `Select(mkIdent(staticModule("scala.Predef")), TermName("println"))` | `reifySymRef` → `staticModule` |
| `List`, `Nil`, `::`, ... (aliases in `scala`'s package object) | `Select(mkIdent(staticModule("scala.package")), TermName("List"))` | the typer resolves the bare name to the alias |
| a member of the `object` enclosing the `reify` (incl. nested objects, classes) | `Select(mkThis($m.staticModule("M").asModule.moduleClass), name)` | `M.this.x` |
| a local / parameter / parameterless `def` / `lazy val` bound outside the body | free term: `val free$x1 = newFreeTerm("x", x, FlagsRepr(<nsc's flags>), "defined by f in F.scala:6:9")`, `setInfo(free$x1, <type>)`, `mkIdent(free$x1)` | `reifyFreeTerm` |
| `this` of an enclosing static class | free term `free$C$this` whose value is `C.this` | `reifyFreeTerm(This(sym))` |
| a written type | rebuilt from the resolved `Type`: `mkIdent($m.staticClass("scala.Int"))`, `AppliedTypeTree`, `Select(mkIdent(staticModule("scala.Predef")), TypeName("String"))`, `mkIdent(selectType(staticPackage("scala").asModule.moduleClass, "AnyRef"))`, `mkIdent(selectType(O.moduleClass, "Inner"))`; a local class by its written path (`outer.D`) | `reifyBoundType` |
| a type parameter / abstract type | with a tag in scope: `mkTypeTree(tag.in[$u.type]($m).tpe)`; otherwise a free type: `newFreeType("T", FlagsRepr(8208), "defined by C in F.scala:6:11")` and `mkTypeTree(TypeRef(NoPrefix, free$T1, Nil))` | `spliceType` / `reifyFreeType` |
| `x.splice` | `x.in[$u.type]($m).tree`, `x` as written | `TreeSplice` |
| a nested `reify` | the call itself, `Apply(Select(<universe>, reify), List(<body>))`; the toolbox that compiles the tree expands it (nsc inlines the inner expansion; the evaluation is the same) | `Metalevels` |
| `while` / `do` | `LabelDef(while$1, Nil, If(cond, Block(body, Apply(Ident(while$1), Nil)), Literal(())))` | the typer's `LabelDef` |
| `s"..."`, `'sym`, `a :: b`, `-x`, `new C { … }`, `(a, b)` | `StringContext.apply(parts).s(args)`, `Symbol.apply("sym")`, `Block(Nil, b.::(a))`, `Select(x, unary_$minus)`, `{ final class $anon …; new $anon() }`, `Tuple2.apply(a, b)` | as `-Ymacro-debug-lite` prints them |

The typer's own artifacts in the typed clone are seen through: `$box` /
`$unbox` wrappers are stripped; an implicit clause the typer appended
(`Array.apply(6, 2, ClassTag.Int)`) is dropped when the callee's type shows a
second clause, so the toolbox infers it again (nsc keeps it and re-infers it
just the same); a materialised `ClassTag.apply($classOf)` becomes
`Predef.implicitly` as nsc's `Reshape.undoMacroExpansion` makes it; a
`def f()` selected without parentheses gets them back
(`Apply(Select(s, trim), Nil)`); applying a *value* selects `apply`.

The expansion shape is unchanged (`crates/typer/src/reify_expand.rs`): a
`TreeCreator` whose `apply` binds `$u` and `$m`, then the free-symbol table
(definitions first, `setInfo`s after, nsc's `SymbolTables.encode` order),
then the tree. When the expression's type mentions a free type, the
`WeakTypeTag[T]` that `Expr.apply` demands is written out as a second
creator sharing the same symbol-table builder; otherwise it is left to the
implicit materialiser as before.

**Free symbol origins.** nsc's `origin(sym)` -- `defined by f in
F.scala:6:9` -- is what the toolbox prints for an unresolved free type, and
the corpus checks it. `Typer::def_spans` records where locals, parameters,
type parameters and local classes were defined; the column is that of the
name.

**The materialiser uses the same type reifier.** `Check::materialize_tag`
asks `Check::reify_type_standalone` first, and only falls back to the
three-shape `TagBody` builder (which is also what names a refusal). That is
how `typeOf[Nest.Inner]`, `typeOf[Bar.type]`, `typeOf[AnyRef]`,
`weakTypeOf[T]` (a free type), `implicitly[WeakTypeTag[Int]]` (a `TypeTag`,
since the reification is concrete -- nsc's `reificationIsConcrete`) and
`showRaw(typeOf[String])` (the `Predef.String` alias) come out as nsc's.

**A codegen bug in the way.** Every `object Test extends App { { val x = 2;
reify { x } } }` of the corpus defines the tree creator inside a
template-level block, and a class defined there that reads the block's
local read it off `this` instead (`NoSuchFieldError: x`) -- reproducible with
no reflection at all (`tests/fixtures/reify2_capture.scala`).
`crate::anon_capture::consider` now applies the rule `lambda_lift::
consider_capture` already had: a term owned by a class that is not among the
class's members is a local of the constructor's frame.

## 3. What was measured

The subset of the corpus these diagnostics named (206 tests: `CORPUS_KINDS="run
pos" CORPUS_SIZE=full`, filtered to the test names): see the slice report
for the before/after numbers per bucket. Fixtures, each run through real
scalac 2.13.16 and scala-rs with identical output: `tests/fixtures/reify2_
free.scala` (free terms, `this` members, definitions, closures, patterns,
`while`, `try`, interpolation, a nested reify, a `for`, an anonymous class),
`reify2_types.scala` (free types and the toolbox's report of them, tags in
scope, written types), `reify2_tags.scala` (the materialiser), `reify2_
capture.scala`, `rf_more.scala`; and `reify2_bad.scala` for what is still
refused (`crates/cli/tests/reify2.rs`).

## 4. What is still refused, and what remains, in order

1. **An assignment to a `var` bound outside the body.** nsc boxes the
   variable (`captureVariable`) and reifies `free$w.elem`; scala-rs carries
   a free term by value, so reads are right and a write is refused
   (`reify_closure6`, `reify_closure7`).
2. **Path-dependent abstract types** (`c.T` for a parameter `c`,
   `reify_renamed_type_spliceable`): nsc's "semi-concrete type member",
   `TypeRef(SingleType(NoPrefix, free$c), selectType(C, "T"), Nil)`.
   `Type::TypeMember` carries no prefix here.
3. **`C#T` through a local class** (`reify_newimpl_30`): the class should
   become a free type; the alias is dealiased instead.
4. **Refinements, existentials, by-name types in a tag or a written type**
   (`reify_magicsymbols`, `implicits-new`, `t6047`, `t6204`): nsc's
   `reifyToughType` with `newNestedSymbol` / `newScopeWith`.
5. **`this` of a class with type parameters**, and members of a class that
   does not enclose the reify.
6. **`import` inside the body**; annotated definitions and parameters
   (`reify_ann*`, which also print the typed tree and would need nsc's
   exact `Reshape` output).
7. **Tree-printing fidelity**: Java nullary methods declared by the prelude
   have no `()` information (`s.length` prints without `List()`); a pattern
   `val` is reified in the parser's desugaring (`x$pat1`) rather than nsc's
   (`x$1` with `@unchecked`); a local class prints `pendingSuperCall` and
   `ScalaDot(AnyRef)` (the `SyntacticClassDef` shape) where nsc prints the
   explicit super call.
8. **Outside `reify`**, the same corpus tests still stop at: whitebox macros
   (22 tests; `crates/typer/src/macros.rs` refuses them at the binding), the
   `Apply` / `Literal` / `NullaryMethodType` extractors inside macro
   implementations that also need `c.reifyTree` / `c.unreifyTree` /
   `c.typecheck` (`macro-reify-chained*`, `macro-sip19*`), `Manifest`s, and
   quasiquote holes of `List[T]` type (`liftList`).
