# The reflect API and `reify` expansion

These notes cover the connected pieces of work on the reflection side of the compiler, in the order they were done. The first is a missing overload in the surface we supply from pickles: `u.Ident(sym: Symbol)` was never installed at all, because erasure of abstract type members was not implemented. The second is support for nested `object`s inside reflect API traits and for writing `<val>.type` as a stable identifier in a type, which together make the tree that `reify` must build compile and run when written by hand. The third is the automatic expansion of `reify { … }` bodies, including the hygiene rules that distinguish it from ordinary quasiquote lowering.

### Missing supply of the `u.Ident(sym: Symbol)` overload (`agent/liftable` remaining, `agent/localcc`)

The bug: `Ident(someSymbol)` was rejected with `no matching overload`, and the candidate list did not even contain a version taking a `Symbol`. The root cause turned out to be that `erased_param_desc` had no case for `Type::TypeMember`, so the abstract type member `Symbol` erased to a "any reference type" wildcard, made the two `Ident` overloads indistinguishable, and caused the `Symbol`-taking one to never be installed in the first place.

```scala
// slick's TableQueryMacroImpl.apply (scala-2/slick/lifted/TableQuery.scala)
Ident(typeOf[Tag].typeSymbol)
```

`Ident` is **not only** the tree factory `val Ident: IdentExtractor` (with `apply(name: Name)`): the `scala.reflect.internal.Trees` trait also declares, under the same name, the convenience method `def Ident(sym: Symbol): Ident` (confirmed with `javap` on scala-reflect.jar 2.13.16: `scala/reflect/api/Trees.class` declares `abstract Trees$IdentApi Ident(Symbols$SymbolApi)` right next to the extractor's `apply`). `Ident(sym)` ought to match the latter, but it was being rejected with `no matching overload for <overload Trees$IdentExtractor | (String)Trees.Ident> with arguments (Symbol)` — that is, with the **`Symbol`-taking version absent from the candidate list from the very start**.

#### Cause

`PickleSupply::install` (`crates/typer/src/pickle_supply.rs`) distinguishes several pickle-derived overloads with the same name and same arity by their parameters' **erased** signature (`erased_param_desc`). At the point where we are looking at the abstract API (`scala.reflect.api.Trees` / `scala.reflect.macros.Universe` — not the concrete `JavaUniverse` that a macro only gets hold of at actual expansion time), `Symbol` is not a concrete class but an **abstract type member** (`type Symbol >: Null <: SymbolApi`), which is converted to `Type::TypeMember`. But `erased_param_desc` had no case for `Type::TypeMember` and fell through to `_ => None` (meaning "a wildcard: any reference type will do"). Since both `Ident(String)` and `Ident(Symbol)` collapsed into the same "one reference" wildcard, `erased_desc` could not settle on a single candidate (`no unambiguous erased descriptor`), and so the `Symbol`-taking version was **never installed at all**.

Real scalac itself erases an abstract type to its own upper bound (or to `Object` when there is no bound). Indeed, the classfile for `scala.reflect.api.Trees.class` carries the concrete descriptor `Ident(LSymbolApi;)LTrees$IdentApi;` (confirmed with `javap`). So I added a `Type::TypeMember` case to `erased_param_desc` that resolves **recursively** to its own `bound_hi` (or `Object` when absent), cutting off at 16 levels in case of cyclic bounds.

#### Verification

A minimal macro implementation, checked with a two-stage compile (the same scheme as `lf2_ctx.scala`). Before the fix, `Ident(c.internal.enclosingOwner)` was rejected with `no matching overload for <overload Trees$IdentExtractor | (String)Trees.Ident> with arguments (Symbol)`; after the fix, `(Symbols.Symbol)Trees.Ident` joins the candidate list and it compiles. The new fixture is `tests/fixtures/lf3_identsym.scala` (prefix `lf`, numbered `lf3` to continue the existing numbering from `agent/liftable`), and the test is `lf3_identsym_supplies_the_symbol_overload_of_ident` in `crates/cli/tests/quasi.rs` (same shape as `lf2_ctx`: it checks that the source compiles and that the result is a classfile that actually loads and verifies under `javap` / `java -Xverify:all`, confirming that the same source also compiles under real scalac 2.13.16). For the actual line in slick itself (`Ident(typeOf[Tag].typeSymbol)` in `TableQueryMacroImpl.apply`), I confirmed from the raw log of `tests/slick_measure.sh` that `no matching overload` is gone.

Since I touched a seam (`pickle_supply.rs`), I ran the brief's mandatory list

`--test overloadshadow --test ambigmap --test setapply --test uniteq --test integral --test ordsummon --test mutcoll --test conform --test e2e`

in full in the foreground and confirmed everything is green.

slick (`tests/slick_measure.sh`) goes from `files=184 errors=223 files_with_errors=60` to `files=184 errors=222 files_with_errors=60`. The `no matching overload … Ident` line for `TableQuery.scala` is gone from the log (the file count itself does not change, because the same file still has other errors unrelated to this fix, such as the unimplemented implicit for `typeOf`).

#### Confirming the same root cause (relation to the `u.WeakTypeTag[T]` / `u.TypeTag.Int` remaining issue)

The brief asked me to check whether the `u.WeakTypeTag[T]` / `u.TypeTag.Int` issue — where they come out as `not a member of JavaUniverse` — has the same root cause. I concluded that it is a **different** root cause. Under `import scala.reflect.runtime.universe._`, `WeakTypeTag[Int]` / `TypeTag.Int` still give `not found: type WeakTypeTag` / `not found: value TypeTag` even after this fix — this is not "the type is found but the overload cannot be narrowed down", it is a different symptom that fails much earlier: the name is not even visible as a wildcard-imported name. This fix (erasure in `erased_param_desc`) is about **narrowing down overload candidates** and does not touch name resolution failures at all. I am leaving it as a remaining issue.

#### Side findings left open (out of scope for this fix)

While working on `Ident(sym)` I also found an unrelated, **separate** bug: under `import c.universe._`, writing `Symbol` as a **bare type annotation** (as in `val sym: Symbol = c.internal.enclosingOwner`) always resolves not to the wildcard-imported `c.universe.Symbol` (the reflection API's abstract type) but to the unrelated `scala.Symbol` that is always in scope (the class of symbol literals like `'foo`), giving `type mismatch; found: Symbols.Symbol  required: Symbol`. A wildcard import should take precedence over the implicit `scala._`, and it does not. slick's actual code (`Ident(typeOf[Tag].typeSymbol)`) does not write an explicit `Symbol` annotation, so it does not hit this side finding and the verification of this fix is unaffected. I have left it as a separate ticket.

### Nested `object`s and `<val>.type` in the reflect API (`agent/reifyd`)

The bug: `c.universe.Expr` reported `value Expr is not a member of Universe` and `Mirror[c.universe.type]` reported `stable identifier required, but c.universe found`. The root causes were that `PickleSupply::complete_named` discarded `MemberKind::Module` entirely (it only read `Def` and `Val` from the pickle), and that `Check::term_path_sym` accepted only `Term | Module | ModuleClass`, dropping pickle-read `val`s (which appear as `SymKind::Method` + `Flags::ACCESSOR`).

I closed holes 1 and 2 of the three that `docs/macros.md` §7.13.4 named as "the holes remaining before our own `reify`". Neither is specific to `reify`; both are **general feature additions**.

* **Supply `object`s nested inside a trait from the pickle.** `trait Exprs { object Expr { … } }` lowers to an interface method `Expr()Lscala/reflect/api/Exprs$Expr$;` plus the module's classfile, but `PickleSupply::complete_named` only reads `Def` and `Val` from the pickle and so was discarding `MemberKind::Module` wholesale. As a result `c.universe.Expr` gave `value Expr is not a member of Universe` and, under `import c.universe._`, `Expr` gave `not found: value Expr` — both **lies as diagnostics**. The accessor is placed on the class where the lookup started, and the call target is left to `erased_desc` to determine (the classfile for `api/JavaUniverse` has `interfaces: 0`, so `invokevirtual JavaUniverse.Expr()` does not resolve). Broken accessors that were merely read from a classfile (return type an unresolved `Type::Named`) are repaired.
* **Allow `c.universe` to be written as a stable identifier in a type.** `Mirror[c.universe.type]` gave `stable identifier required, but c.universe found`. The cause was that `Check::term_path_sym` accepted only `Term | Module | ModuleClass`, so a `val` read from the pickle was dropped (in a classfile a `val` cannot be distinguished from a zero-argument `def`, so it comes through as `SymKind::Method` + `Flags::ACCESSOR`). The reader for `Type::SingleType` unwraps a zero-argument `Method` to its result type via `SymbolTable::singleton_underlying`.

Along the way I fixed three holes where **compilation silently succeeded and the program failed at runtime**.

* A method's **parameter symbols** were visible as "members" of that method (the `qual.sym` fallback in `Check::type_select`). `m.staticClass(n).fullName` resolved to `staticClass`'s parameter `fullName`, and codegen emitted a `Fieldref` whose owner class was the method's erased descriptor, giving `ClassFormatError: Illegal class name "(Ljava/lang/String;)L…;"`.
* A parenless member selection was missing the `checkcast` to `declaring_class` (the `Apply` path has one). `u.Expr` gave a `VerifyError`.
* The receiver of a member `object` was being discarded and the enclosing source class's `this` pushed instead (`gen_module_member_receiver`). `universe.Liftable[String](f)` gave `ClassCastException: Main$ cannot be cast to scala.reflect.api.Liftables`.

I also **hand-wrote `Exprs#Expr.apply`**. The expansion of `reify` ends by calling `c.universe.Expr.apply[T](mirror, creator)`, but in the pickle the signature's first parameter is `Mirror[Universe.this.type]`, and that `this.type` is converted relative to the class being completed (the module `Expr$` itself), so it became `Mirror[Expr$]` and matched no call at all. This is the same reason `ensure_tag_module` hand-writes `TypeTag.apply`, so I gave it the same treatment (`install_expr_apply`, with the erased descriptor written out by hand too). The implicit clause is kept, so `WeakTypeTag[T]` is filled in by the existing materialiser.

With this, **the tree that `reify` ought to build works end to end when written by hand**.

There are two sets of fixtures.

* `tests/fixtures/rd_nested.scala` — against the runtime universe, uses nested `object`s both through a path and through a wildcard import, plus `Mirror[scala.reflect.runtime.universe.type]`, printing 5 lines. It **matches real scalac 2.13.16**.
* `tests/fixtures/rd_impl.scala` + `tests/fixtures/rd_use.scala` — the form that `reify { 42 }` / `reify { RdHelper.twice(x.splice) }` ought to expand to, hand-written with `TreeCreator`, and **actually expanded and run by the engine** (static symbols via `mirror.staticModule`, splices via `Expr.in`). A separate test pins down that compiling and running the same two files under real scalac in two stages produces the matching `42 / 42 / true`. A creator that gets the receiver or the universe wrong still compiles, so only comparing the output can catch it.

The tests are 4 new ones added to `crates/cli/tests/engine.rs`.

`tests/slick_measure.sh` is unchanged at `errors=134 → 134` and `files_with_errors=48 → 48`, and `tests/slick_subset.sh` is unchanged at `38 files / 204 classes / verified=204 failed=0`. slick's two macros stop where `reify` is required, and this slice only got things working up to that point.

#### Remaining

* **The expansion of `reify { … }` itself is still unimplemented**, and the diagnostic is still the one from `docs/macros.md` §7.8. The materials for the tree are now in place, so what remains is the synthesis plus **hygiene** (lowering static symbols to `mkIdent(mirror.staticModule(...))`, `splice` to `x.in(m).tree`, and rejecting locals by name). nsc's expansion form is recorded from actual measurement in `docs/macros.md` §7.14.
* Writing a nested ***class*** inside a trait as a **type** (`u.Liftable[Int]`) is still `not found: type Liftable`. What I added here is only the term side.
* Since the upper bound of `u.Mirror` (`api.Mirror[self.type]`) cannot be read from the pickle, inside a creator you have to cast to `scala.reflect.api.Mirror[u.type]` (nsc writes `u.Mirror`).

### Expanding `reify { … }` (`agent/reifybody`)

The bug: `reify { … }` could not be expanded at all — slick's `TableQueryMacroImpl` failed with `cannot expand reify`. The root cause was simply that the expansion was unimplemented; `agent/reifyd` had already made the target tree work when hand-written, so this work makes the compiler build that tree automatically, with the hygiene rules the reification is there to provide.

I made the compiler **build automatically** the tree that `agent/reifyd` had gotten as far as "works end to end when hand-written" ([`docs/macros.md`](../macros.md) §7.15). `reify { … }` is expanded by `crates/typer/src/reify_expand.rs` into

```text
{ final class $treecreator1 extends scala.reflect.api.TreeCreator {
    def apply[U <: scala.reflect.api.Universe with Singleton](
        $m$untyped: scala.reflect.api.Mirror[U]): <Trees.TreeApi> = {
      val $u = $m$untyped.universe
      val $m = $m$untyped.asInstanceOf[scala.reflect.api.Mirror[$u.type]]
      <the body, lowered into universe calls>
    }
  }
  <universe>.Expr.apply[T](
    <universe>.rootMirror.asInstanceOf[<api.Mirror>], new $treecreator1()) }
```

The lowering of the body uses the same `crates/typer/src/reify.rs` as quasiquotes, but **differs exactly by the amount of hygiene required** (`Reifier::in_reify`).

* A static `object` becomes `$u.internal.reificationSupport.mkIdent($m.staticModule("..."))`. It resolves by **symbol** rather than by the name as written, so the meaning does not change even if the same name exists in the scope the expansion lands in.
* `x.splice` becomes `x.in[$u.type]($m).tree`. The creator rebases onto the mirror it was handed, so the result belongs to the same universe as the surrounding tree.
* **Type arguments** become `mkTypeTree(...)`. The contents are built from the same materials used to build a `TypeTag` (`crate::materialize::TagBody`): a monomorphic class becomes `$m.staticClass(...)`, a type constructor `appliedType`, and a type parameter `tag.in[$u.type]($m).tpe` from **the tag in scope**. That last one is what slick's `reify { TableQuery.apply[E](cons.splice) }` needs.
* **Locals, parameters, `this`, blocks, type annotations, and type arguments without a tag are rejected by name.** nsc turns locals into *free terms* and carries them along with the expansion, but scala-rs cannot build that. Building them as bare names would compile and run, but would point at **whatever name happens to exist at the call site** — precisely the bug that reification exists to prevent.

What each identifier is gets decided by `Check::reify_refs`, which **speculatively types a clone and rolls it back** (the same as `hole_lifts`). For types, the whole body is speculatively typed once in order to obtain the `T` of `Expr.apply[T]`, and `WeakTypeTag[T]` is filled in by the materialiser of §7.10. So that the materialiser can find the universe even for `c.universe.reify { … }` (without `import c.universe._`), that universe is pushed as an import prefix for the duration of typing the expansion.

`Typer` did not have the source string (quasiquotes passed `Reifier` a string they had built themselves). Since the body of `reify` is **text from a real file**, I added `typecheck_units_src` and pass it in from the driver. `Reifier` needs it to distinguish `A => B` from `Function1[A, B]`, and `(a, b)` from `Tuple2(a, b)`.

There is one fixture set plus one negative fixture.

* `tests/fixtures/rb_impl.scala` + `tests/fixtures/rb_use.scala` — writes as macro implementations: 4 kinds of literals / application of a static `object` / `.splice` (one, two, `String`, `Boolean`) / `c.universe.reify` / type arguments (`Int`, plus one and two type parameters resolved from tags), compiles in two stages and prints **16 lines**. Compiling and running the same two files in two stages under real scalac 2.13.16 gives **the same 16 lines**. The last two lines fill the splices with side-effecting expressions, so the count changes if the tree drops a splice or builds it twice.
* `tests/fixtures/rb_bad.scala` — the 5 forms that are rejected (parameter / local / type annotation / block / type argument without a tag). Real scalac accepts all 5, so this is **a confession of what is unimplemented**.

The tests are 3 new ones added to `crates/cli/tests/engine.rs` (`rb_reify_expands_and_runs` / `rb_reify_matches_real_scalac` / `rb_reify_gaps_are_named`).

`tests/slick_measure.sh` goes `errors=115 → 113` with `files_with_errors=41 → 41`. slick's `reify { TableQuery.apply[E](cons.splice) }` in `TableQueryMacroImpl` **can now be expanded**, and the two errors `cannot expand reify` plus the `cannot expand apply` it dragged along with it are gone. Since I did not touch `crates/backend/`, I skipped `tests/slick_subset.sh`.

#### Remaining

* The `value apply is not a member of TableQuery[E]` that remains on the same line is the remaining issue from §7.13 (overload selection for `TableQuery.apply`) and is unrelated to reify.
* **Inferred type arguments** at the call site still do not reach the macro (remaining issue 1 of §7.13), so `rb_use.scala` writes the type argument out explicitly as `RbUse.idOf[Int](5)`.
* *Free terms* for locals and parameters, blocks, function literals, `this`, and type annotations are all unimplemented (diagnosed by name).

### `currentMirror`, `runtimeMirror`, and nested types of the reflect API (the `agent/reflectruntime` slice)

The scala/scala test corpus's `test/files/run/` has roughly 200 failures whose
diagnostic is not a wrong answer but a missing name: `value currentMirror is
not a member of package scala.reflect.runtime` (147), `not found: type
TypeTag` (16), `not found: type WeakTypeTag` (6), `type Transformer is not a
member of Universe` (6), `not found: value runtimeMirror` (5), plus a handful
of `<notype>` variants. None of these are about macro expansion or `TypeTag`
materialisation (both separate, already-tracked jobs); the names were never
*installed* at all, on receivers this project can otherwise already reify.

Three independent roots, all in the "supply" layer:

1. **Nested classes of the reflect API were never installed as types.**
   `PickleSupply::complete_type_member_uncached` (`crates/typer/src/
   pickle_supply.rs`) only recognised a pickled member of kind `TypeAlias` or
   `AbstractType`; a `MemberKind::Class` hit (a nested trait or abstract
   class — `TypeTags.TypeTag`, `TypeTags.WeakTypeTag`, `Trees.Transformer`,
   and the rest of the reflect API written the same way) fell through to
   `None`. This is the *type* half of the gap `agent/reifyd` closed the *term*
   half of (§7.13 item 1, nested `object`s); `docs/macros.md` line 1132 and
   this file's own §"Confirming the same root cause" section had already
   named it and left it open. Fixed by resolving the hit's owner + name
   through `PickleSupply::ensure_class`, the same way `install_type_alias`
   resolves an alias's target. This alone fixed all 16 + 6 + 6 occurrences of
   `TypeTag` / `WeakTypeTag` / `Transformer` above (`u.Liftable[Int]` — a
   nested *class*, not a trait — is presumably fixed the same way, but was
   not in this corpus slice's numbers and was not separately verified).

2. **`JavaUniverse#runtimeMirror(ClassLoader)` — a completely ordinary
   method with real bytecode — had no parameter type to install against.**
   `java.lang.ClassLoader` is a plain JDK class with no `ScalaSignature`, and
   `PickleSupply::ensure_class` refuses to build a symbol for a class outside
   `scala.` that has none (this exact gap was already named in this file's
   `runtimeMirror` note and in `materialize.rs`'s doc comment). Fixed the
   narrow way, not the general one: `crates/typer/src/
   prelude_reflectruntime.rs` declares `java.lang.ClassLoader` and
   `Class#getClassLoader(): ClassLoader` by hand, the same way `java.lang
   .Class` itself is already hand-declared in `prelude.rs`. This fixed all 5
   + 1 occurrences of `runtimeMirror`.

3. **`currentMirror` leaves no bytecode at all**, because it is one of nsc's
   own *fast-track* macros (`scala.tools.reflect.FastTrack`,
   `scala/reflect/runtime/package.scala`: `def currentMirror: universe.Mirror
   = macro ???`) — the compiler recognises it by the macro symbol's full name
   and never even looks at the pickled `@macroImpl` annotation, which on the
   real classfile is the placeholder `???`, not a usable reference. The
   general "install a method by matching its erased descriptor against the
   owner's classfile" path (`PickleSupply::install`) can therefore never
   install it — confirmed by `SCALA_RS_PICKLE_DEBUG=1`, which showed
   `scala/reflect/runtime/package$#currentMirror/0: no unambiguous erased
   descriptor (want [])`, i.e. zero bytecode candidates, not a real ambiguity.
   `PickleSupply::install_known_macro` supplies exactly this one binding, by
   name, the same way nsc's own `FastTrack` table does: `scala.reflect
   .runtime.Macros$.currentMirror(c: blackbox.Context): c.Expr[universe
   .Mirror]` is a real, ordinary blackbox macro implementation with real
   bytecode (confirmed with `javap scala.reflect.runtime.Macros$` against
   scala-reflect.jar 2.13.16), so the *type* of `currentMirror` is read from
   the same pickle as always, and the existing JVM-bridge engine
   (`crates/typer/src/expand.rs`) is left to decide whether it can actually
   expand the call. It cannot yet — `c.reifyEnclosingRuntimeClass` is not
   among the `Context` methods the engine implements — so every reference
   still fails, but now with the honest, existing "macro expansion is not
   implemented: cannot expand currentMirror (implementation scala/reflect
   /runtime/Macros$.currentMirror)" diagnostic (`Typer::report_macro_calls`)
   in place of "not found". This fixed the visibility half of all 147 + 5 + 2
   occurrences of `currentMirror`; actually expanding it is future work,
   scoped to whoever next extends the engine with `reifyEnclosingRuntimeClass`
   (and `c.abort`, which the real implementation also calls).

Measured on the exact corpus subset these four diagnostics named (239 `run`
tests, `CORPUS_KINDS=run CORPUS_SIZE=full`, filtered to the affected test
names): `pass=0` before, `pass=3` after
(`macro-reify-typetag-notypeparams`, `macro-reify-typetag-typeparams-tags`,
`typetags_multi`). The other 236 still fail — most now one or two layers
deeper, behind `mkToolBox`, `reify` of blocks/locals, or `TypeTag`
materialisation, all of which are separate, already-scoped jobs. **The count
was genuinely open before running it**, in both directions: supplying a name
can turn out to unlock nothing (the next wall is immediate) or a great deal
(three full passes from one visibility fix each), and this measurement is
what settled it rather than the a priori estimate.

Fixtures: `tests/fixtures/rt_typetags.scala` (`TypeTag[Int]` /
`WeakTypeTag[String]` as types, `Transformer` as a type, `runtimeMirror`,
compiled and run, matching real scalac 2.13.16 — comparing `.tpe.toString`
rather than the tag's own `toString`, because nsc's `WeakTypeTag` materialiser
upgrades a concrete type to a full `TypeTag` regardless of which one was
asked for, which is a materialisation nuance this slice does not touch) and
`tests/fixtures/rt_currentmirror_bad.scala` (a confession: real scalac
accepts and runs it, scala-rs gives the honest "not implemented"
diagnostic). Both are new tests in `crates/cli/tests/engine.rs`
(`rt_typetags_resolve_and_run`, `rt_typetags_matches_real_scalac`,
`rt_currentmirror_is_named_not_stubbed`) rather than `e2e.rs`: every fixture
here needs scala-reflect.jar on the classpath, and `engine.rs` is where that
support (`scala_reflect_jar`, `compile`, `run_main`, the real-scalac diff)
already lives, the same as `rd_*` / `rb_*` before it.

`Manifest` / `ClassManifest` (10 occurrences in the assigned bucket) is a
separate, older API (`scala.Predef.Manifest`, pre-dating `TypeTag`) that was
deliberately left alone: it needs its own implicit materialisation for
arbitrary types, unrelated to the `TypeTag` machinery `materialize.rs`
already has, and is a small enough slice of the total (10 of ~200) that
building it was judged not to pay for itself here.

### `currentMirror` expanded, the toolbox reached, and four supply bugs (the `agent/toolbox` slice)

The bucket this slice was given was the rest of the runtime-reflection API in
`test/files/run/`: 128 tests whose first diagnostic was one of `mkToolBox is
not a member of JavaUniverse.Mirror` (44), `no matching overload for
(Mirrors.RuntimeClass)Symbols.ClassSymbol` (16), `value prefix is not a member
of blackbox.Context` (16), `cannot expand currentMirror` (14), the
`ModuleSymbol` variant of the `RuntimeClass` one (9), and `not found:
extractor Apply` (10). **Measured on exactly that subset** (`CORPUS_KINDS=run
CORPUS_SIZE=full`, filtered to those test names): `pass=0` before, `pass=38`
after. Nothing here was a `TypeTag` or `reify` change; those buckets are
untouched and are what most of the remaining 90 now fail on.

Six independent roots, none of them in the reflection API as such -- every one
is a general rule the compiler had wrong, and reflection is simply where the
standard library exercises it hardest.

1. **A named import off a *value* never asked the pickle.** `import
   c.{prefix => prefix}` reported `value prefix is not a member of
   scala.reflect.macros.blackbox.Context` while `c.prefix` written out in the
   same file resolved, because `Check::type_select` calls
   `supply_from_pickle` and `Check::import_named` did not. The type half of
   `import_named` had already been made unconditional for the same reason
   (`Database`/`Session` in gitbucket); this is the term half.

2. **A package's own class hid the term its package object declares.**
   `import scala.tools.reflect.ToolBox` names *two* things -- the trait
   `ToolBox` and, in `scala.tools.reflect.package`, the implicit conversion
   `def ToolBox(m: ru.Mirror): ToolBoxFactory[ru.type]`. The trait alone
   satisfied the selector, so the conversion never entered scope and
   `mirror.mkToolBox()` was `value mkToolBox is not a member of
   JavaUniverse.Mirror`. `import_named` now also asks the package object when
   nothing it found is in the term namespace.

3. **A concrete alias in a more derived class lost to the abstract
   declaration it overrides.** `scala.reflect.api.Mirrors` declares `type
   RuntimeClass >: Null <: AnyRef`; `scala.reflect.api.JavaUniverse` refines
   it to `type RuntimeClass = java.lang.Class[_]`.
   `PickleSupply::self_type_member` walked the linearisation looking only for
   an *abstract* member, walked past the alias, and installed the opaque one
   -- so `classSymbol(classOf[A])` was `no matching overload for
   (Mirrors.RuntimeClass)Symbols.ClassSymbol with arguments (Class[A])`. The
   linearisation is most-derived-first, so asking each step for an alias
   before an abstract member is nsc's own rule.

   That fix alone made the member *un*installable, because the signature and
   the JVM descriptor are then erased in two different vocabularies: the
   declaration in `Mirrors.RuntimeMirror` erases to
   `classSymbol(Ljava/lang/Object;)`, which is what the class file says, while
   the caller must satisfy `Class[_]`. `PickleSupply::install` now keeps the
   caller's view as the member's type and, *only when the first descriptor
   search fails*, recomputes the erasure at the declaration site
   (`decl_site_want`) to find the bytes to call.

4. **`currentMirror` is expanded by the compiler, the way nsc's `FastTrack`
   does it.** The previous slice supplied the *binding*
   (`install_known_macro`) and left the expansion to the JVM bridge, which
   cannot run it: the implementation calls `c.reifyEnclosingRuntimeClass`,
   whose result is a `Literal(Constant(<a type>))` the reply protocol has no
   node for. `crates/typer/src/fasttrack_mirror.rs` builds the expansion
   directly. What it builds was **read off real scalac 2.13.16** with
   `-Ymacro-debug-lite`:
   `_root_.scala.reflect.runtime.universe.runtimeMirror(this.getClass.getClassLoader)`.
   The implementation's own `c.abort("call site does not have an enclosing
   class")` is kept as a diagnostic.

   Two smaller things were in the way of it ever being tried.
   `install_known_macro` did not set `supplied_macro_def`, which is the gate
   on the typer walking applications looking for something to expand at all
   -- so a run whose only macro was this one reported "cannot expand" *with no
   reason attached*, because nothing had tried. And
   `expand_macro_application` returned early for any `Type::Method` receiver,
   which a *parameterless* macro def keeps: `def currentMirror: Mirror` is
   `paramss: []` and the bare identifier already is the application. `def f()`
   is `[[]]` and is still excluded.

5. **A `$default$n` getter was kept twice.** `adopt_binary_class` replaces the
   class-file reader's crude member with the pickled one for every member
   name, but it skips names containing `$` -- and a default getter is only
   ever reached through `complete_named`'s `synthetic_ok` path, which did not
   replace anything. `ToolBox.typecheck$default$2` was then both `(): Object`
   and `(): TypecheckMode`, and filling in the default at `tb.typecheck(t)`
   was `ambiguous overload`.

6. **A compound upper bound offered only its first parent's members.**
   `SymbolTable::class_sym_of` answers with one symbol and takes the first
   parent of a `Type::Refined` that is a class. `scala.reflect.api.Names`
   declares `type TypeName >: Null <: TypeNameApi with Name`, where
   `TypeNameApi` is empty (it exists to give `TypeName` an erased identity)
   and everything a name can do comes from `Name` through `NameApi`.
   `Check::members_through_compound_bound` searches every parent, after the
   ordinary search and the pickle have both found nothing.

   This one was found by *slick*, not by the corpus: fix 3 makes
   `symbolOf[R].name` the `TypeName` nsc gives it instead of the abstract
   `SymbolApi.NameType` (whose bound is plain `Name`), and
   `mapToImpl`'s `rSym.name.toTermName` in `ShapedValue.scala` then had
   nowhere to resolve. `tests/slick_measure.sh` went from `errors=0` to
   `errors=1` on fix 3 alone and back to `errors=0` with fix 6 -- the reason
   to run the compile measures before committing rather than after.

Separately, in the **backend**: `crates/backend/src/pickle.rs` wrote
`Type::Nothing` and `Type::Null` as `Any`, because both fell through to the
catch-all. `scala.reflect.runtime` reads that pickle, not the class file's
erased descriptor (which said `scala/runtime/Nothing$` all along), so `class A
{ def foo = ??? }` reflected back as `def foo: Any`. That is four of the
corpus's `t5256*` tests and, more to the point, a wrong signature in every
class file scala-rs emits.

Fixtures and tests are `tests/fixtures/tb_reflect.scala` (the mirror, both
`RuntimeClass`-taking methods, the reflected `Nothing`, `mkToolBox().eval`,
and `typecheck`'s five defaults -- matching real scalac 2.13.16),
`tb_prefix_impl.scala` + `tb_prefix_use.scala` (a two-stage macro that reaches
`c.prefix` through a named import, also matched against real scalac), and
`tb_bad.scala` (a confession: see below). They are 5 tests in
`crates/cli/tests/toolbox.rs`, which is its own file because the toolbox needs
scala-compiler.jar on the classpath as well as scala-reflect.jar.

`tests/fixtures/rt_currentmirror_bad.scala` -- the previous slice's confession
that `currentMirror` could not be expanded -- is now
`tests/fixtures/rt_currentmirror.scala`, an ordinary fixture whose output is
compared against real scalac. `println(currentMirror)` is not comparable (a
mirror's `toString` carries the class loader's identity hash), so it prints
`currentMirror == runtimeMirror(getClass.getClassLoader)` instead.

#### Remaining, in this bucket

* **`scala.reflect.api.Mirror`'s members are unreachable** (`staticClass`,
  `staticModule`, `staticPackage`; `tb_bad.scala`). It is an abstract *class*
  reached through the parent `Mirror[JavaUniverse.this.type]`, and that
  parent does not convert -- `PickleSupply::conv_at` has no reading for a
  singleton type of the enclosing class -- so `api.Mirror` is not in a
  mirror's linearisation at all. `classSymbol` / `moduleSymbol` are declared
  by the ordinary trait `Mirrors.RuntimeMirror` and are unaffected.
* **A nested class's constructor result is pickled with the package as its
  prefix**, not the enclosing class: `class A` inside `object Test` reflects
  back as `def <init>(): A` where nsc says `Test.A`.
  `Pickler::ctor_result_type` takes `class_pkg`, which is derived from the
  JVM name. Four `t5256*` tests differ by that line alone.
* `not found: extractor Apply` (10 tests) is pattern matching against the
  reflect API's tree extractors. Those tests also need `c.reifyTree`,
  `c.unreifyTree` and subclassing `Transformer`, so the extractor is the
  first wall of several.
* The `macro-term-declared-in-*` tests that still differ do so only in the
  *spelling* of the prefix tree (`Expr[Nothing](Test.this.outer.Macros)`),
  which is what the engine is handed by the Rust side.

#### Two `neg` tests that were passing for the wrong reason

Supplying names turns a `neg` test that this compiler *happened* to reject
into one it accepts, and the corpus's `neg` pass rate counts a rejection for
any reason at all. Diffing the full `neg` pass set against a throwaway branch
cut from `main` (658 tests either side) named exactly two:

* `macro-invalidusage-methodvaluesyntax` was rejected with "cannot expand
  foo", which stopped being true once a parameterless macro def could be
  expanded at all. The real rule is nsc's **`macros cannot be eta-expanded`**:
  `Macros.foo _` has nothing to take a reference to, because a macro def has
  no bytecode. `Typer::reject_macro_eta` reports it, recognising the eta
  position by the `Type::Method` expectation `Check::type_eta` types the
  operand with.
* `macro-override-method-overrides-macro` was rejected because
  `import c.{prefix => prefix}` in its *implementation* file did not compile.
  The real rule is **`macro can only be overridden by another macro`**, which
  `override_check.rs` now checks in the one direction the base's macro-ness
  makes certain (a macro base, a non-macro override).

Both diagnostics match nsc's wording and line, so the two tests pass again
*and* the log's `neg` scoring (`tests/scala_corpus_report.sh`) now scores them
as the right rejection rather than an accidental one. The final `neg` pass set
is identical to `main`'s, test for test.

### Blocks, and members of static `object`s in a `reify` body

`docs/macros.md` §7.17 has the whole account. In short: `reify { println("a") }`
and `reify { println("a"); println("b") }` were both refused, and both are now
built the way nsc builds them — `println` as
`Select(mkIdent($m.staticModule("scala.Predef")), TermName("println"))`, a block
as `Block(init, last)`. The rule is the one the rest of reification already
follows: **resolve by symbol, refuse what cannot be found again through a
mirror.** A member of the *enclosing* `object` is still refused, because nsc
uses `mkThis(...asModule.moduleClass)` there and the two print differently; so
is a definition inside a block, which needs nsc's `newNestedSymbol`.

The measured result on the cluster this was scoped from — the 163 scala/scala
`run` tests whose first diagnostic mentions `reify` — is **`pass=0` before,
`pass=0` after**, with the symptoms moved one or two layers deeper. That number
is the useful finding, not a disappointment to explain away: **147 of the 163
need a toolbox at run time** (`.eval` or `currentMirror.mkToolBox`), which no
amount of reify work reaches. Whoever picks up this cluster next should scope
the toolbox (`c.reifyEnclosingRuntimeClass` in the engine, and implicit search
finding `scala.tools.reflect.Eval` in scala-compiler.jar's package object)
before scoping more reify shapes.

Three more (`macro-reify-basic`, `macro-reify-unreify`,
`macro-undetparams-macroitself`) now compile and fail at run time on a
*separate*, already-recorded bug: scala-rs does not write `macro_impl` into its
own pickle (`docs/macros.md` §7.16 "What remains", item 3), so a macro **def**
it compiled in an earlier round is read back as an ordinary method and the call
site emits a real invocation. Reproducible with no `reify` in the program at
all; `macro-reify-basic` is one implemented `@macroImpl` pickle away from
passing.

One `neg` test moves the other way: `neg` goes 658 → 657 because
`test/files/neg/macro-cyclic` stops being rejected. It was passing for the
wrong reason — we refused `c.universe.reify { implicitly[SourceLocation] }`
because `implicitly` was unclassified, while nsc rejects it as a *cyclic
reference* (the only implicit candidate is the `implicit def sourceLocation =
macro impl` being type-checked). Reifying `implicitly` as a `Predef` member is
right; scala-rs then finds that candidate and accepts the file, because it has
no counterpart to nsc's cyclic check. This is exactly the caveat the corpus
harness documents about `neg` being an upper bound.
