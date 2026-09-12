# Type prefixes for inner classes

Branch `agent/prefixtypes`, module `crates/typer/src/prefix.rs`, tests
`crates/cli/tests/pfx.rs` (`tests/fixtures/pfx_*.scala`).

## The problem

nsc's `TypeRef(pre, sym, args)` carries a *prefix*: the enclosing instance an
inner class belongs to. scala-rs's `Type::Class { sym, args }` does not, and
three families of programs depend on it:

1. **Prefix-sensitive conformance.** `a.In` and `b.In` are different types for
   two stable `a`, `b: Outer`; `Outer#In` is the supertype of every `p.In`;
   `def f(x: p.S1)` is not implemented by `def f(x: S1)` written in another
   class (`neg/abstract-class-2`, "their prefixes (i.e., enclosing instances)
   differ"); `MailBox#Message` does not conform to `in.Message`
   (`neg/t1010`). All of these were accepted.
2. **Outer type arguments.** The members of `o.In` for `o: Outer[String]` are
   written in `Outer`'s vocabulary, and `T` is instantiated by the prefix:
   `new B[X].m(1).n(x)` (cats `syntax/semigroupal.scala`), `class KeySet
   extends MySet[K]` inside `MapOps[K]` used as a `MySet[Int]` through `HM[Int]`
   (library `HashMap`), `new o.In(Some("s"))`. All of these were rejected.
3. Everything that follows from 1 and 2: implicit search over inner classes,
   pattern matching on `o.Rec(...)`, `Outer#Session` dealiasing, tags.

## Representation

No new `Type` variant and no new field on `Type::Class`: 1101 sites match on
`Type::Class { sym, args }` across 101 files, and several slices edit them
concurrently. Instead the prefix rides on the *as-seen-from view* that `A#B`
projections already use -- a `Type::Refined { parents: [Class], decls }` whose
decls start with the bookkeeping entry `AS_SEEN_FROM_MARK` -- as one more
bookkeeping decl:

```
RefineDecl::Type { name: PREFIX_MARK ("<prefix>"), rhs: Some(prefix), .. }
```

The prefix is one of

* `ThisType(C)` -- a bare `In` written inside `C` (nsc `C.this.In`), `this.In`,
  a self alias, or the `C.this` the override checker reads members through;
* `SingleType { prefix, sym }` / `ModuleRef` -- `p.In` on a stable path. Unlike
  `singleton_to_type`, `Typer::singleton_prefix_of` keeps the whole chain
  (`x.p.In` and `p.In` are two types), and an object nested in a class keeps
  its own prefix (`p.O` is `SingleType { p, O$ }`);
* any other type -- the projection `P#In`, or a *widened* receiver (a value
  whose path this compiler did not keep: `unstable.mk` for `def mk = new In`).
  nsc skolemizes the latter (`_1.In forSome { val _1: Outer }`); here it is
  the projection, which conforms to strictly more types on the right-hand
  side and to nothing more on the left (a singleton on the right still
  requires the same singleton);
* `NoType` -- unknown (an enclosing instance a path does not reach). Conforms
  to anything, in either direction.

A `Type::Class` for an inner class with **no** view is an unknown prefix --
every jar signature and pickled type, every type built before this module --
and conforms exactly as it did before: prefixes are compared only when both
sides carry one. Everything downstream of the typer keeps seeing the class:
`as_seen_from_view` strips the view for erasure, the pickle, descriptors and
the macro engine's tag wire.

Only classes `SymbolTable::is_inner_class_of_class` says yes to get a prefix:
a `SymKind::Class` (class or trait) whose owner is a class or trait, neither
flagged `JAVA` -- or a class read from a class file that
`is_binary_nested_class` recognises: `SymbolTable::binary_read` (set by
`apply_java_class_meta`), not `STATIC`, a `$` in its JVM simple name, owner a
class. Members of objects have one enclosing instance, local classes none,
Java nested classes are static. A pickled signature's `THIStpe` prefix on an
alias or a method result (`alias_prefix` / `result_prefix`, `SigType::This`)
becomes a `ThisType` view (`with_pickled_this_prefix`), so `c.mk` on a
separately compiled `class C { class D; def mk: D }` is a `c.D`
(`tests/fixtures/pfx_binlib`).

## Where prefixes are attached

* `Typer::this_prefixed` (check_types.rs): a bare `In` resolved by
  `tree_to_type`'s `Ident` arm, and `new In`, get `ThisType(C)` for `C =
  ident_prefix_class(owner)` -- the innermost enclosing class that has the
  class as a member.
* `Typer::projected_class_type`: `P#In` gets `P`.
* `Typer::path_dependent_type`: `p.In` gets `singleton_prefix_of(p)`, and the
  member is read through `project_from_prefix_at(.., at)`, which rewrites the
  `C.this` inside what it reads (an alias's right-hand side: `JdbcBackend#Session`
  is `JdbcBackend#JdbcSessionDef`, `p.Session` is `p.JdbcSessionDef`).
* `SymbolTable::subst_as_seen_from_at` (symbol.rs), on every selection:
  1. `rewrite_view_this`: `C.this` in a view's prefix becomes the receiver's
     prefix when `C` is the receiver's class or an ancestor (`o.mk` for `def mk:
     In` is an `o.In`), the receiver *type*'s own prefix `n` levels out when
     `C` encloses the receiver's class (`database.capabilities` on `database:
     JdbcBackend.this.JdbcDatabaseDef[F]` is `JdbcBackend.this.DatabaseCapabilities`),
     the path's prefix for an object nested in a class (`p.O.f` takes a
     `p.S1`), and stays `E.this` through the receiver's own `this`.
  2. the ordinary base-class walk over the class under the view;
  3. `attach_inner_prefixes`: every bare inner class of a class the walk went
     through gets the receiver (`canonical_prefix`: the stable path the
     member was selected on when `check_select` has one, else `C.this` for a
     class at its own parameters, else the receiver type itself);
  4. the view's prefix is walked in turn, with its own `seen` set, which is
     what instantiates the enclosing class's parameters.
* `apply_types` (check_namer.rs) and `apply_context_bound` (check.rs) apply
  type arguments under a view; the constructor path (check_apply.rs), the
  parent-constructor path (check_member.rs) and the `copy` rewrite
  (check_select.rs, `resolved_class_tpt_at`) keep the head's prefix on the
  result; `ctor_outer_prefix` reads it back for constructor parameters.
* Parents are stored bare (`parent_form`), because every reader of `parents`
  matches `Type::Class`; the prefix a parent was written with is kept in
  `SymbolTable::parent_prefixes` and read by the subtype check below.

## Conformance (`is_sub_type`)

* Two views of the same class: `prefix_conforms(p1, p2) && core1 <: core2`.
  `prefix_conforms` is nsc's `isSubPre`: `NoType` on either side conforms; a
  singleton on the right admits only the same singleton
  (`same_singleton_prefix`); anything else is a projection, and `widen(p1) <:
  p2` decides. `same_singleton_prefix` is deliberately lenient where this
  compiler cannot tell, and only there:
  * `norm_singleton` first: an object is one value however spelled (`O.type`,
    `ModuleRef`, `this` inside `O`), a self alias is its class's `this`, and a
    path whose *last step's declared type, read as seen from the path*
    (`path_step_type`), is a singleton denotes that value (`r.ring` with
    `ring: C` on `r: Poly[BigInt.type]` is `BigInt`, pos/t5777);
  * two `this` prefixes of classes related by inheritance are the same (the
    override checker reads both members through the subclass);
  * two paths are the same when they name one symbol, two method parameters
    of one name (dependent method types matched positionally, run/t6135), or
    two members of one name in classes related by inheritance (a member and
    its override: `new Dep { val a = new A; val b = a.mkB }` against `Dep`'s
    `val b: a.B`, pos/t5313 -- nsc's `equalSymsAndPrefixes`), under prefixes
    that are themselves the same;
  * a value's path on the *left* against its class's `this` on the right
    (`p.X` against `P.this.X` with `p: P`): a member declared bare in `P` and
    read through `p` means `p.X` in nsc, and this compiler does not rewrite
    every `P.this` on every road a type travels (an alias expanded after the
    selection, an implicit view's result, a dependent method's result). The
    other direction stays rejected -- scalac: `found P.this.S1, required
    P.this.p.S1` -- as do two different values (`a.In` against `b.In`) and a
    base-class value against a subclass's `this` (neg/abstract-class-2).
* A view on the left against another class: the class first, then its parents
  read through the prefix (`prefixed_parents_conform`) -- `hm.KeySet <: MySet[Int]`.
* A bare class on the left against a view on the right of a different class:
  the parents, each with the prefix it was written with (`parent_prefixes`) --
  `new S1 {}` inside `P` is a `P.this.S1`, not a `p.S1`; `class C extends o.In`
  is an `o.In`.
* The override checker's `certainly_different` (override_check.rs) treats two
  views of one class behind two different singleton prefixes as different
  types, which is what rejects `neg/abstract-class-2`.

`base_type_instance` (check_pattern.rs), the implicit unifier `unify_at`
(implicits.rs) and `collect_expected` (check_infer.rs) read a view's base
types through its prefix, so `refl: A =:= A` fitted to `hm.KeySet <:<
MySet[?T]` solves `?T = Int`. `collect_type_parts` adds the prefix's parts to
the implicit scope (SLS 7.2), so `StrTypes.BCT[String]` sees `object
StrTypes`'s implicits.

`display_type` prints a singleton prefix the way nsc does (`a.In`,
`Outer.this.In`, `O.In`); a projection prints bare.

Everything that asks "which class is this, and what shape" reads through the
view (`prefix::strip_view`): `sam_sig` (a function literal against an inner
SAM trait, pos/t11558), overload scoring and `shape_arity`, `is_function_shaped`,
`align_to_param_class` / `align_arg_to_param` (a `Some[t1.View]` lined up with
`Some[t1.Two[A, A]]`, run/t9114), `view_shape`, `base_type_instance`, the
pattern binder kinds (pos/t4070), the divergence check's `complexity` /
`head_sym` (a shrinking derivation through an inner witness class was read as
one node and cut off, pos/t8146b), `type_mentions_tparam`, `tag_wire` and
`type_to_wire` (a `c.typecheck` answer, pos/t9392).

Implicit scope: `collect_type_parts` adds a view's prefix's parts (SLS 7.2)
and records, per search (`ImplicitMemo::companion_prefixes`), the prefix each
inner class's companion was reached through; `implicit_candidate_ty` then
reads such a companion member as seen from that prefix, so `object Inner {
implicit def fromOther(b: Bridge): Inner }` found for an `o1.Inner` gives an
`o1.Inner` (pos/t4947), and `at_import_prefix_of` does the same for `import
o1.Inner.fromOther`. Reached through two paths in one search (`b: s.E` with
`b: r.E`, pos/t5340) the member is read bare, which conforms to either.

## What landed (this branch)

* neg/abstract-class-2, neg/t1010, neg/sabin2 rejected for nsc's reason;
  `a.In` vs `b.In`, `Outer#In` vs `a.In`, `Outer[Int]#In` vs `o.In`,
  dependent-method arguments, `new S1 {}`/`class C extends S1` against
  `p.S1`, `new Fixed {}.Inner` all rejected on scalac's lines
  (`pfx_bad.scala`, `pfx_override_bad.scala`).
* Outer type arguments through inner classes: method results, path
  constructors, `extends MySet[K]`, nested-in-nested, aliases, generic inner
  classes, higher-kinded outers, implicit conversions and context bounds on
  inner traits, implicit search through inner classes, pattern matching on
  `o.Rec(n, t)`, anonymous subclasses with explicit type arguments and mixins
  (`pfx_inner.scala`, 17 lines of output identical to scalac's).
* Measures (904b32c5 + agent/catsrest): library 165/58 -> 153/55, cats
  4/3 -> 2/2 (`syntax/semigroupal.scala` 71/78 were the `B[X]#B1[A]` case),
  gitbucket 88/43 -> 88/43 with three fewer error sites (the `Migration`
  override errors) and no new one, slick 0 errors / 1504 classes.
* The outer-instance miscompile handed over by agent/gbmacro (`import
  prof.api._; class Mine(k) extends Inner(k)` passed the wrong `$outer`,
  `ClassCastException`): `import_prefixed` / `applied_alias_prefixed` /
  `qualify_inner_ctor_head` give a `new` or parent head reached through an
  import, an alias or a path its prefix, `new_prefix_instance` evaluates it,
  and the gitbucket-shaped `gbtable` reduction prints the same DDL as scalac
  under `-Xverify:all`.
* Separately compiled inner classes through a value prefix (`new c.D`, `c.mk`,
  `class Sub extends c.D(4)` against a scalac-built library:
  `tests/fixtures/pfx_binlib`, `pfx_binlib_use.scala`), with `gen_new` taking
  the outer from the class file's `InnerClasses` attribute
  (`binary_outer_desc`).
* The 15 pos/run corpus tests the first prefix commit lost (t5313, t5340,
  looping-jsig, t5958, t4947, t11174b, t8138, t11558, t12520,
  t2421_delitedsl, t4070, t5777, t8146b, t9392, run/t9114) pass again, each
  by one of the view-reading rules above.

## Probe battery

`/private/tmp/scala-rs-pfx/probes/p01..p74` (run with `run.sh` / `battery.sh`;
scalac and scala-rs verdicts compared, run output compared under
`-Xverify:all`). 67 of the 72 agree with scalac (p58's "difference" is the
stack trace of the same exception). The ones that do not, each reduced:

| probe | shape | status |
|---|---|---|
| p09 | `IntIsIntegral.mkNumericOps(6) / 2` (inner class read from the jar) | open, step 1 below |
| p35 / p48 | `o.Rec(3, "z")` -- companion `apply` of an inner case class through a path, *without* `.apply` | open, step 2 |
| p39 / p49 | `class C extends a.In` -- `VerifyError` in `C.<init>` | pre-existing codegen gap (same on 904b32c5) |
| p40 (original form) | `new Abstract[F](..)` accepted | pre-existing: no "class is abstract; cannot be instantiated" check |
| `new c.api.D` (left out of pfx_binlib_use.scala) | a pickled alias `type D = In` of a separately compiled class, through a value | open: `type D is not a member of Aliases` -- the pickle lookup of an alias member through a value prefix |

## Remaining steps

1. **Inner classes read from a jar.** `binary_read` + `is_binary_nested_class`
   now recognise a non-static nested class of a Scala class file, and the
   pickle's `THIStpe` prefixes on aliases and method results are kept. Still
   open: `check_select` reads a bare inner-class result type before the stub
   is completed (the class file is only read when a member is looked up on
   it, one selection too late for `IntIsIntegral.mkNumericOps(6) / 2`, p09),
   and an *alias* member of a separately compiled class through a value
   (`new c.api.D`, `type D is not a member of Aliases`) is not found by the
   pickle lookup.
2. **`o.Rec(...)` sugar.** `resolve_overload_inner` (check_overload.rs) sees
   `fun_ty = ModuleRef(Rec$)` and reads `apply` raw. `type_apply` should hand it
   `SingleType { o, Rec$ }` when the callee is a stable `Select` on an object
   nested in a class, and a `SingleType` arm there should read `apply` with
   `subst_as_seen_from_at(ModuleRef(m), Some(&fun_ty), ..)` -- the walk already
   handles that shape (`subst_as_seen_from_walk_at`, "object nested in a
   class, selected through a path"). `.apply` written out and `new o.Rec` work.
3. **Existential skolems.** A widened receiver's prefix is its type, read as a
   projection. nsc's skolem also rejects `unstable.mk` against `Outer.this.In`
   inside `Outer`; a `BoundedWildcard { hi: Some(C) }` prefix would express
   that (`is_singleton_prefix` false, `widen_prefix` = `hi`). Deliberately not
   done: it only adds rejections.
4. **Pickling.** Views are pickled as the bare class (`backend/pickle.rs`).
   Writing `THIStpe` / `SINGLEtpe` prefixes would make separately compiled
   inner-class signatures round-trip; combined with step 1 it closes the loop.
5. **Display.** `P#In` prints as `In`; printing the projection would match nsc
   but changes existing fixture messages.
6. The pre-existing codegen gaps above (`class C extends a.In`, `copy` on an
   inner case class from outside its enclosing instance) and the missing
   abstract-instantiation check are independent of the representation.
