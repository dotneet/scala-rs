# The reflect API and `reify` expansion

These notes cover three connected pieces of work on the reflection side of the compiler. The first is a missing overload in the surface we supply from pickles: `u.Ident(sym: Symbol)` was never installed at all, because erasure of abstract type members was not implemented. The second is support for nested `object`s inside reflect API traits and for writing `<val>.type` as a stable identifier in a type, which together make the tree that `reify` must build compile and run when written by hand. The third is the automatic expansion of `reify { … }` bodies, including the hygiene rules that distinguish it from ordinary quasiquote lowering.

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

I made the compiler **build automatically** the tree that `agent/reifyd` had gotten as far as "works end to end when hand-written" ([`docs/macros.md`](docs/macros.md) §7.15). `reify { … }` is expanded by `crates/typer/src/reify_expand.rs` into

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
