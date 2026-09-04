# Application chains, new, and copy

These notes cover a family of bugs that all come down to the same thing: an application chain has to be looked at as a whole, not one `Apply` clause at a time. They cover `super` and self-types, the bound type of an `x @ Extractor(...)` pattern, curried `copy`, and curried `new C(…)(…)`, together with the parser and backend fixes those required. A final chapter collects a batch of smaller slick fixes: named arguments on a qualified companion, return-type inheritance for `override def`, `copy` rewrites that lost their symbol, and function literals passed to SAM parameters. Every chapter records what was measured on slick before and after.

### `super` and self-types, the bound type of `x @ Extractor(...)`, and curried `copy` (`agent/tail3`)

Three distinct slick failures: `c.volatileHint` was rejected on a `case c @ LiteralNode(_)` pattern, `computeCapabilities` was reported as a recursive method needing a result type, and `t.copy(identity = x)(t.profileTable)` produced `value apply is not a member of TableNode`. The roots turned out to be a bind pattern that never narrowed its bound variable's type, a `super` lookup that walked self-types (plus an `emit_super_accessors` call missing from the `object` code path), and a `copy` rewrite that fired on the innermost `Apply` of a curried chain before the outer one had been considered.

The assignment was the single-occurrence and cascade clusters among slick's remaining errors (most frequent first). Tests live in
`crates/cli/tests/tail3.rs`, fixture prefix `t3`.

Measurement went from `files=184 errors=203 files_with_errors=60` to
**`files=184 errors=184 files_with_errors=57`** (-19 errors / -3 files).

| Cluster | before | after |
|---|---|---|
| `value volatileHint is not a member of Node` | 3 | **0** |
| `recursive method computeCapabilities needs result type` | 3 | **0** |
| `value apply is not a member of TableNode` | 3 | **0** |
| `value getDumpInfo is not a member of TypeGenerator` | 2 (a by-product of the same root) | **0** |
| `value getOrElse is not a member of Product` | 4 | 4 (**not fixed**, for the same reason as `agent/tail1`) |

#### 1. `x @ Extractor(...)` must narrow the bound type

`slick/jdbc/{DerbyProfile,JdbcStatementBuilderComponent,SQLServerProfile}
.scala` all write `case c @ LiteralNode(_) if c.volatileHint => …` (or the
same shape with a `:@` in between) against a scrutinee of type `Node`.
`volatileHint` exists only on `LiteralNode` (not a `case class`, but an
ordinary class whose companion carries a hand-written
`def unapply(n: LiteralNode): Option[Any]`), not on `Node`. Real scalac binds
the `x` of `x @ Extractor(...)` to **the receiver type the extractor itself
declares** (the same implicit type test as `case x: T`), but
`crates/typer/src/check.rs` (the `unapply` branch of `type_pattern`) always
left the type of the pattern as a whole (`pat.ty`) **as the scrutinee's
type**, so `c` stayed `Node` and `c.volatileHint` was rejected.

The fix is confined to the **inside** of `TreeKind::Bind`: after typing the
inner pattern, narrow only the bound variable's type, using a new
`unapply_receiver_type` (which unifies the extractor's declared parameter type
against the scrutinee the same way `subst_unapply_tparams` does). The `pat.ty`
of the `TreeKind::UnApply` node itself is deliberately left **as the
scrutinee's type** -- `gen_unapply_pattern` in `crates/backend/src/gen.rs`
reads it to decide whether the runtime `instanceof` test is redundant
(`is_sub_type(pat.ty, param_ty)`), so narrowing it here as well would make that
check always true and **delete the test itself**. Indeed, with a
typecheck-only version, running `describe(new OtherNode)` (which should match
neither `LiteralNode` case) produced `ClassCastException: OtherNode cannot be
cast to LiteralNode` -- a concrete instance of the brief's procedure (run under
`-Xverify:all` and diff against real scalac's stdout before trusting anything)
actually paying off.

#### 2. `super` must not walk self-types

`slick/{jdbc/DB2Profile,relational/RelationalProfile,sql/SqlProfile}.scala`
all override `computeCapabilities` (with no return type annotation) as
`super.computeCapabilities ++ …Capabilities.all`. The base
(`BasicProfile.computeCapabilities: Set[Capability] = Set.empty`) has an
explicit type, so this should never be a real cycle -- following the brief, I
first ran a minimal reproduction through real scalac before digging in
(`t3_super_chain.scala` compiles under scalac on the first try).

Two causes were stacked here:

* **Typechecking**: in `RelationalProfile extends BasicProfile with
  RelationalTableComponent with … with RelationalActionComponent`,
  `RelationalActionComponent { self: RelationalProfile => }` is the first
  parent `super` picks (under `super_target`'s old "use the last parent"
  heuristic). `SymbolTable::lookup_member` (ordinary member lookup) also walks
  self-types, which is correct for `this.foo` and unqualified references from
  **inside** the body of a self-typed trait, but per SLS 6.7.3 `super` never
  goes through a self-type (only real inherited parents).
  `super.computeCapabilities` inside `RelationalProfile` came back, through
  `RelationalActionComponent`'s self-type, to `RelationalProfile`'s **own**
  not-yet-completed override -- a genuine cyclic reference, but for a different
  reason than the one nsc reports. I added
  `SymbolTable::lookup_member_real` (a version that does not walk self-types)
  and `Typer::super_select_member` (which walks `this_id`'s real parents,
  later declarations first, looking for the first parent in the real
  inheritance chain that has `name`), and swapped them in inside `type_select`
  only when the qualifier is `Super`.
* **Backend**: with the above fixed, typechecking passed, but `ClassImpl` (an
  ordinary `class`) and `ObjectImpl` (an `object`) behaved differently --
  `ObjectImpl.m` threw `AbstractMethodError: … Mid$$super$m() of interface Mid`.
  `emit_class` in `crates/backend/src/gen.rs` calls `emit_super_accessors`
  (which implements, on the concrete class being mixed into, the abstract
  `Trait$$super$m` accessors that a trait's `super.m` calls need), but
  `emit_module` (the separate code path used only for `object`s) **never**
  called it. Every `object Foo extends SomeTrait` inheriting a trait that
  calls `super` in its own body is affected -- exactly slick's per-database
  profile objects, `object H2Profile extends JdbcProfile` and friends -- but
  the typechecking bug in (1) always rejected them first, so not one of them
  had ever compiled. The fix is one added line in `emit_module`:
  `self.emit_super_accessors(&mut b, cls);`.

#### 3. `p.copy(...)( ...)` has to look at the whole chain first

`slick/ast/Node.scala` has `final case class TableNode(schemaName, tableName,
identity, baseIdentity)(val profileTable: Any)` -- a curried `case class`
whose second parameter list holds a single `val`. The actual use sites
(`slick/compiler/{AssignUniqueSymbols,EmulateOuterJoins}.scala`) write
`t.copy(identity = x)(t.profileTable)`, with the same two parameter lists as
the constructor.

`Typer::try_rewrite_case_copy` (`crates/typer/src/check.rs`) rewrites
`p.copy(…)` directly into a constructor call (so that it rides on the existing
constructor-call inference rather than reimplementing `copy[T]`'s own type
inference). That function is called on **one `Apply` node at a time**, so for
`t.copy(identity = x)(t.profileTable)` it fired first on just the **inner**
`Apply` (`t.copy(identity = x)`) -- before the outer `Apply` (the one passing
`(t.profileTable)`) had even been considered -- filled in **all** fields,
including those belonging to the second list, with `t`'s own values, and
returned a finished `TableNode`. The outer `(t.profileTable)` was then read as
an `.apply` call on that `TableNode` value, giving "value apply is not a
member of TableNode". (Whether the type really stays curried in the bytecode
or is flattened into a single parameter list -- javap alone cannot tell you,
since Scala methods with multiple parameter lists, constructors included, are
**always** erased to a single JVM method -- was settled before touching
anything, by confirming with real scalac 2.13.16 that `r.copy(a = 2)(r.extra)`
does in fact compile.)

I added `Typer::try_rewrite_case_copy_curried` and try it at the **head** of
`try_rewrite_case_copy`. It peels the `Apply` chain down to the selection of
`copy`, and if there are two or more levels, rebuilds a call sequence
`ClassName(list1)(list2)…` matching the constructor's real parameter-list
shape. (Note that it uses the companion's `apply` rather than `new C(…)(…)` --
a curried `new` call had **its own**, narrower overload-resolution hole, in
that it only ever looks at one `Apply` layer at a time, so routing through it
would just have traded one bug for another. If peeling yields fewer than two
levels (`depth < 2`) it does nothing and defers to the existing single-list
version, so the overwhelmingly common non-curried case is untouched.)

#### Verification

`t3_extractor_bind.scala` / `t3_super_chain.scala` / `t3_curried_copy.scala`
all pass `-Xverify:all` under both `--scala-library` and `--no-scala-library`,
and are diffed against real scalac 2.13.16's stdout
(`crates/cli/tests/tail3.rs`). All three are confirmed to be rejected by
`main` before the fix. Since I touched shared seams (`check.rs` / `symbol.rs`
/ `gen.rs`), I ran `--test tail3 --test conform --test e2e` in the foreground
and confirmed `cargo test --workspace` is green too.

#### Remaining

* `value getOrElse is not a member of Product` (4 occurrences): the same
  symptom `agent/tail1` already tried and failed to reduce to a standalone
  reproduction (the lub of `if (rs.wasNull) None else Some(r)` in
  `nextBlobOption() getOrElse(…)` collapses to `Product` for exactly four
  types: `Blob` / `Array[Byte]` / `Clob` / `Object`). I again built several
  reduced versions from scratch, but they **compile** both under real scalac
  and under our binary, and the dependence on the state of all 184 slick files
  is unchanged from what `tail1.rs` recorded. No new leads.
* `no matching overload for (=> F[B])(FlatMap[F])F[B]` (3 occurrences, cats'
  `>>` extension method), `value map is not a member of Any` (3 occurrences),
  and `value flatMap is not a member of Async$` / `value effect is not a member
  of <notype>` / `value database is not a member of BasicBackend.Session` /
  `value reduceLeft is not a member of Option[Node]` (2 each) could not be
  investigated in the time available.
* **A separate bug found as a by-product**: a curried `new C(…)(…)`
  (a direct constructor call, not via `copy`) emitted
  `ambiguous overload for apply with arguments (String)` at
  `slick/lifted/SimpleFunction.scala:74`, on `new SimpleLiteral(name)(tpe)`
  (a symptom that predates this slice's changes). Fixed in `agent/tail4`.
  The root was not "each `Apply` layer resolved independently" but **the
  parser not putting `New` at the head of the chain**. See "A curried
  `new C(…)(…)` is one constructor call" below; that is also where
  `try_rewrite_case_copy_curried`'s avoidance of rebuilding via `new` gets
  fixed.

### A curried `new C(…)(…)` is one constructor call (`agent/tail4`)

`new SimpleLiteral(name)(tpe)` was rejected with `ambiguous overload for apply with arguments (String)`, and the root turned out to be the parser: it decomposed only one `Apply` layer, so `New` ended up in the middle of the chain rather than at its head, and the type position of `New` held an application expression. Fixing that exposed three further holes, and a separate root -- `SymbolTable::lub` walking past candidates whose class matched but whose type arguments differed -- accounted for the long-standing `value getOrElse is not a member of Product`.

Tests live in `crates/cli/tests/tail4.rs`, fixture prefix `t4`.

Measurement went from `files=184 errors=177 files_with_errors=57` to
**`files=184 errors=166 files_with_errors=53`** (-11 errors / -4 files).

| Cluster | before | after |
|---|---|---|
| `value getOrElse is not a member of Product` | 4 | **0** |
| `value apply is not a member of ConstColumn[T]` / `TypedCase[B, P]` / `ConnectionArbiter$` | 3 | **0** |
| `ambiguous overload for apply with arguments (String)` | 2 | **0** |
| `recursive method apply needs result type` | 1 (a cascade from the same root) | **0** |
| `type mismatch; found: Option[Product] required: Option[Option[Any]]` and other `Product`-derived errors, 3 in all | 3 | **0** |

(Two newly reachable errors have appeared: type mismatches on `Shape[…]` /
`Tuple2[T, T2]` in `slick/lifted/Query.scala`. They are the result of lines
that used to fail earlier now getting through.)

Following up on what `agent/tail3` left behind as an "unfixed bug" --
`new SimpleLiteral(name)(tpe)` at `slick/lifted/SimpleFunction.scala:74`
(`ambiguous overload for apply with arguments (String)`) -- the root turned out
to be **in the parser, not in overload resolution** (1). Fixing that exposed
two holes that only then became reachable (2 and 3), plus one **silent
miscompile** caused by `tail3`'s `copy` rewrite avoiding `new` (4).

As a second, independent root, I fixed `value getOrElse is not a member of
Product`, which four slices had failed to reduce, recording it as "dependent
on the state of all 184 slick files" (5). It does not depend on slick at all;
the cause was `SymbolTable::lub` walking straight past candidates whose class
matched but whose type arguments differed.

#### 1. Root cause: `New` was not attached to the **head** of the chain

`parse_new` (`crates/parser/src/parse.rs`) decomposed only **one level** of
the parent `Apply` (`Apply(Apply(C, a), b)`) and wrapped its `fun` (i.e.
`C(a)`) in `New`. So `new C(a)(b)` became `Apply(New(C(a)), b)`, putting an
**application expression** in `New`'s "type" position. Typing a `New` types
that position as an ordinary expression, so `C(a)` became an `apply` lookup --
`ambiguous overload for apply` for `SimpleLiteral`, whose companion has its own
`apply`, and `no matching overload for constructor apply` for classes that do
not. (`tail3`'s observation that "each `Apply` layer is resolved
independently" was a restatement of the symptom; what was really going on is
that `New` sat in the wrong place.)

`parse_new` now peels the chain all the way and puts `New` at the head
(`new C(a)(b)` becomes `Apply(Apply(New(C), a), b)`), and
`Typer::flatten_curried_new` (`crates/typer/src/check.rs`) does the same thing
`type_parent_ctor_app_in` has always done for `extends A(1)(2)` -- flatten the
argument lists into one, but only when the head is a `New`. Both `pick_ctor`
and the JVM treat a constructor's parameter lists as flat, so this is where
the two paths converge.

But flatten only **as much as the constructor selected by the first list can
take**. For `class Foo(a: Int) { def apply(b: Int) = … }`, nsc reads
`new Foo(1)(2)` as `(new Foo(1)).apply(2)`, and flattening the two lists would
**construct a two-argument `Foo`** -- silently, if the class happens to have
such a constructor. Since it is the length of the first list that decides
which constructor is being built (for `class Ov(a: Int) { def this(a:
Int, b: Int) = … }`, `new Ov(1)(2)` is the one-argument one), we take the
total argument count from candidates whose first clause length matches,
falling back to candidates whose first clause is longer (to account for
omitted defaults and implicits). Both are covered in `t4_curried_new.scala`.

#### 2. Read the constructor's clauses with the type arguments written on the `new`

`slick/lifted/Case.scala:21`'s
`new TypedCase[B, P](ConstArray(cond, res.toNode))(bType, om.liftedType(bType))`
passes a `BaseTypedType[B]` to a clause declared as `TypedType[B]`. That
conformance only holds once the class's type parameters have been substituted
with `[B, P]`, but the `new` path was calling `pick_ctor` (the version that
does not pass type arguments). `extends A(1)(2)` has passed them via
`pick_ctor_at` from the start. I made the two match.

#### 3. Do **not** search again for an explicitly written implicit clause

Constructor arguments arrive at `fill_defaults_and_implicits` already
flattened, whereas the constructor **symbol**'s `paramss` still has two
clauses, so the second clause was read as "not yet filled" and the search
results were appended **after** the arguments the user wrote. `new
K[B]("s")(tb)` typechecked and then produced bytecode passing three arguments
to a two-parameter constructor, at which point `java -Xverify:all` reports
`VerifyError: Bad type on operand stack` -- a miscompile, not a diagnostic.
Filling now happens only when the call really is **short**
(`args.len() < ctor_params.len()`).

#### 4. `copy()(x)` is a `new`, not the companion's `apply`

`tail3`'s `try_rewrite_case_copy_curried` went through the companion's `apply`
because curried `new` was broken. But the two are the same method **only when
the companion is synthetic**. `emit_module`
(`crates/backend/src/gen.rs`) does not emit a synthetic `apply` if the
companion's body declares even one `apply`. `SimpleLiteral` is exactly that
case, so `def rebuild = copy()(buildType)` compiled into a call to a method
that is not in the classfile (`NoSuchMethodError: SimpleLiteral$.apply(String,
Type)` -- a path only reachable once (1) was fixed). nsc's `copy` is a
constructor call outright, so I changed it to build `new C(…)(…)`.

#### 5. `lub` walked past candidates whose class matched but whose type arguments differed

`value getOrElse is not a member of Product` (4 occurrences,
`slick/jdbc/PositionedResult.scala`) is the symptom that four slices --
`agent/tail1` / `mismatch10` / `mismatch11` / `tail3` -- all failed to reduce,
recording it as "dependent on the state of all 184 slick files". In fact it
does not depend on slick **at all**. What it depends on is how much of
scala-library that run had loaded.

`SymbolTable::lub` (`crates/typer/src/symbol.rs`) walked `a`'s base type
sequence and returned the **first** candidate `b` conforms to. For
`if (rs.wasNull) None else Some(r)` that sequence is `None.type`,
`Option[Nothing]`, and after that `Option`'s own parents.
`Some[Blob] <: Option[Nothing]` is false (`Blob` is not a subtype of
`Nothing`), so it moves on to the next candidate -- but `scala/Option`'s
classfile says `implements scala.Product`, so if anything in that run had read
`scala/Option`'s classfile, `Product` is already lined up as an upper bound and
the walk stops there. The function goes on to walk `b`'s sequence as well, and
getting that far would have found `Option[Blob]`.

What was being walked past were candidates whose **class matched but whose
type arguments differed**. The two sequences meet at `Option`; one side simply
had `Nothing` and the other `Blob`. So now, if `b`'s sequence has an entry for
the **same class**, we join the type arguments (simply rethrowing it at the
"same class, join the arguments" branch `lub` already has for itself) and stop
the walk at that type. The answer becomes `Option[Blob]`, independent of how
much of the library has been read.

I also tried a version that collects every candidate and "ranks them by
specificity", but that is **wrong**: in `lub(Circle, Rect)` both `Product` and
`Shape` are minimal, and because `Product <: Equals`, `Product` ends up looking
"more specific". Note that nsc's answer is precisely `Option[Blob] with Product
with Serializable`; we do not go as far as building the intersection type.

`t4_lub_bases.scala` writes this shape out in user code
(`sealed abstract class Opt[+A] extends Marker` / `case object Nn extends
Opt[Nothing]` / `case class Sm[+A](v: A) extends Opt[A]`), so it does not
depend on library loading state and fails on plain `main` with
`value get is not a member of Product`.

#### Verification

`t4_curried_new.scala` / `t4_lub_bases.scala` pass `-Xverify:all` under both
`--scala-library` and `--no-scala-library`, and are diffed against real scalac
2.13.16's stdout (`crates/cli/tests/tail4.rs`). Both are confirmed to be
rejected by `main` before the fix. `t4_curried_new_bad.scala` pins down that
flattening has not become a "let anything through" -- a third argument list, a
type mismatch in the second list, evidence that cannot be filled in --
(nsc 2.13.16 emits the same three errors). Since I touched the seam between
the parser and `check.rs`, I ran `cargo test --workspace`.

slick: `errors=177 files_with_errors=57` to `errors=166 files_with_errors=53`.
The subset stays at `38 files / 204 classes / verified=204 failed=0`.

### Four small clusters in slick's remaining 155 errors (`agent/tail5`)

Four unrelated small bugs, none of which matched the brief's guesses: named arguments on a qualified companion failed because a module symbol has no `paramss`, `override def f = ...` did not inherit its return type from the overridden member, `recv.copy(...)` built its `new C(...)` by bare name and so needed `C` to be lexically in scope, and function literals passed to SAM-typed parameters never matched during overload scoring. Fixing the second one also uncovered two knock-on problems that only showed up when measuring across all of slick.

Tests live in `crates/cli/tests/tail5.rs`, fixture prefix `t5`.

Measurement went from `files=184 errors=155 files_with_errors=52` to
**`files=184 errors=149 files_with_errors=49`** (-6 errors / -3 files).

Every one of the brief's guesses was partly or entirely wrong (the same
pattern as previous slices). The four roots I actually established are these.

#### 1. Named arguments on a qualified companion

`pkg1.Bar(a = 1, b = "x")` (qualified) produced "unimplemented syntax: named
arguments (method parameters not resolved)", while `Bar(a = 1, b =
"x")` (unqualified) had worked from the start. The cause is that `fun.sym`
differs. The unqualified form resolves to the `apply` method itself, but the
qualified form resolves to the **module** `Bar` -- `rewrite_receiver_apply`
deliberately does not rewrite qualified companion references (the codegen for
`scala.Some(1)` depends on that). A module symbol has no `paramss` of its own,
so `first_clause_ids` found nothing.

I added a branch to `named_arg_param_ids` that, when `fun.sym` is a `Module`,
reads the parameter names from that module's `apply` member. This is the same
thing the overload callee already does. Fixtures: `t5_named_qual(_bad)`.

#### 2. `override def f = ...` inherits its return type

`override def run(n: Node) = n match { case Wrap(x) => run(x) ... }` produced
"recursive method run needs result type" even though it overrides
`def run(n: Node): Any = ...`. Following SLS 6.1 -- "if the overriding
definition does not write its own type, it is taken to have the type of the
overridden member" -- the return type should be known before inference even
starts. A method of the same shape that overrides nothing
(`t5_override_infer_bad.scala`) still gets this error, exactly as under real
scalac 2.13.16 -- only the overriding case was wrong.

`type_def_sig` (only when the `override` modifier is present) now walks the
ancestors via `overridden_ret_type`, looking for a member of the same name and
same parameters whose return type is already known, and borrows it. Only the
return type is borrowed; the body is checked and inferred exactly as written.

Behind the direct fix there were two side effects that isolated reproductions
never revealed (they only turned up once I measured across all of slick):

- **The borrowed type has to be re-read "as seen from the overriding
  class".** The first version returned the ancestor's declaration as-is, which
  is right for non-generic overrides but, for generic ones, produced a flood of
  `type mismatch; found: T required: T` where the same letter merely denoted
  different symbols. It now substitutes via `subst_as_seen_from` (the same one
  `bind_found` / `type_select` use for inherited members).
- **Members whose type had become "known" were still sitting in the
  lazy-completion queue.** `register_typed_sig` only looked at the parse syntax
  (whether a `: T` was written), so it left the symbol in `pending_sigs`
  regardless of the type having been determined some other way. A
  **self-reference** inside the body would then call `complete_lazy_sig` on
  itself mid-flight, lock the symbol, re-enter `type_def_body` on a copy of the
  very body being typed, and the self-reference inside that copy would this
  time find the locked symbol and report a bogus cyclic reference.
  `register_typed_sig` now treats a `DefDef` whose return type is already known
  as no longer lazy.
- **`overridden_ret_type` initially force-completed not-yet-completed ancestor
  candidates on the spot via `complete_lazy_sig`.** That ran the candidate's
  body (and any forward references that body makes) at a point where the
  top-down pass over **the candidate's own declaring file** had not yet
  registered that file's real scope (imports included), so names visible only
  through those imports got resolved by the "owner chain" fallback and reported
  "not found: value X" at a fabricated, unrelated span (measuring on slick
  turned `errors=155` into `errors=307`; most of them were `not found: value
  Capability` / `DumpInfo` appearing inside files that do in fact import them
  correctly). I changed it to simply skip not-yet-completed candidates as if
  they had not been found, and to keep searching further up that candidate's
  own ancestors. That is enough -- every case that genuinely applies ends up at
  an ancestor whose return type is written explicitly, so nothing has to be
  forced.

Fixtures: `t5_override_infer(_bad)`.

#### 3. `recv.copy(...)` built `new C(...)` by **name**

`try_rewrite_case_copy` rewrites `recv.copy(f = v)` into `new C(...)`, but it
built the head of that `new`'s type as a bare `Ident { name: "C" }` -- even
though the caller already holds `C`'s real `SymbolId` (`class_sym_of` on the
receiver's type), it made **ordinary lexical name resolution** find `C` all
over again when typing the rewritten tree. For a class reached only through an
inheritance chain in another file, one that the file calling `.copy()` does
not import by simple name, there is no reason for that name to be in scope.
This showed up as a "not found: type C" with no line or column (the
synthesized tree has no real span). slick's
`override def getDumpInfo = super.getDumpInfo.copy(...)` in
`slick.jdbc.BaseResultConverter` never imports `slick.util.DumpInfo`, and was
exactly this.

The fix was just to set `sym` / `ty` directly on the `Ident` this rewrite
synthesizes, from the `SymbolId` we already know, and to make the code that
types a `New` use them as-is instead of re-resolving by name when they are
already set.
Fixtures: `t5_case_copy_qual(_bad)`.

#### 4. Function literals for SAM (not literal `FunctionN`) parameters

For `case class Builder(sql: String, setParameter: SetParameter[Unit])`
(where `SetParameter[-T] extends ((T, PositionedParameters) => Unit)`), a call
`Builder(sql, (u, pp) => ...)` failed to match at all during overload scoring.
The machinery that pre-types a function literal into the parameter shape the
callee expects (nsc's `pretypeArgs`, our `agreed_lambda_params`) only runs for
a genuine `Overload` with two or more candidates, and `Builder(...)` has just
the one `apply` synthesized for the case class, so it did not qualify and the
literal went into scoring still as `(<notype>, <notype>) => <notype>`. Even if
it had been typed, `arg_score`'s rule for function parameters only recognized a
literal `scala.FunctionN` and would not accept a trait that merely inherits
one. slick's `SQLActionBuilder(sql, (u, pp) =>
...)` with `case class SQLActionBuilder(sql: String, setParameter:
SetParameter[Unit])` is the same shape.

The only thing I changed is `arg_score`: if a class-typed parameter is
SAM-convertible (`SymbolTable::sam_sig`), compare against the function type
its abstract method represents. That is the same treatment literal `FunctionN`
already had. Since an existing rule makes a literal of undetermined type match
any function-shaped parameter while its parameters are open, no separate
pre-typing was needed once scoring itself could see through the SAM. I also
tried extending `agreed_lambda_params`' pre-typing to single candidates, but
reverted it -- measuring across slick, pre-typing then also applied to
single-candidate signatures whose own type parameters are not yet determined,
such as cats-effect's `Async[F].uncancelable[A](body: Poll[F] => F[A]): F[A]`,
fixing a wrong (undetermined) type before the call's own inference had
resolved `A`, and causing far more regressions than the `arg_score`-only fix
(the literal gets typed correctly either way -- `adapt_args_to_params`, which
runs after the real (and here only) candidate is settled, re-types every
argument against the actual parameter types). Fixtures: `t5_sam_ctor(_bad)`.

`t5_sam_ctor` is verified only under `--scala-library`. `SetParameter`
inherits `Function2`, but our private runtime (`--no-scala-library`) currently
emits only `scala.Function0` / `scala.Function1`; that is an independent,
pre-existing hole with nothing to do with named arguments, overriding, or SAM
conversion (confirmed: even a minimal reproduction containing only
`val f: (Int, Int) => Int = (a, b) => a + b` fails under
`--no-scala-library`, its output being `NoClassDefFoundError: scala/Function2`).
I left it out of this slice and split it off as a separate item.

#### Verification

All four positive fixtures pass `-Xverify:all` under both `--scala-library`
and `--no-scala-library`, and are diffed against real scalac 2.13.16's stdout
(`t5_sam_ctor` under `--scala-library` only, for the reason above). All four
are confirmed to fail on `main` before the fix. The four negative fixtures pin
down that none of the repaired paths has become a "let anything through"
(an unknown parameter name, recursion that overrides nothing, an arity
violation on a SAM parameter), and real scalac 2.13.16 rejects the same four.
Since I touched `crates/typer/src/check.rs` and `crates/typer/src/lazysig.rs`
(the lazy-signature-completion seam), I ran `--test tail5 --test tail3 --test
tail4 --test conform --test e2e` (553 tests) plus the supply seam checklist
(`--test overloadshadow --test ambigmap --test setapply --test uniteq
--test integral --test ordsummon --test mutcoll`) in the foreground.
All green. `cargo fmt --all -- --check` reports no diff, and the warnings from
`cargo clippy --workspace --all-targets --release` are exactly identical
before and after (zero new warnings).

#### Remaining

"quasiquote q"..." (a hole of type `<error>` is not lifted)" (3 occurrences,
`slick/lifted/ShapedValue.scala`) was a cascade, just as the brief said --
the root has nothing to do with quotes or macros: `rTag.tpe.decls.collect(...)`
at `slick/lifted/ShapedValue.scala:42` is reported as
`value collect is not a member of Scopes.MemberScope`. Real scala-reflect's
`ScopeApi` (checked with `javap`) inherits `scala.collection.Iterable[SymbolApi]`,
so `collect` should be there, but scala-rs's reflection API prelude / pickle
completion is not finding it. `MemberScope` is not defined by scala-rs itself
-- we read the pickle from the scala-reflect jar directly -- so this is another
hole in pickle completion (`pickle_supply.rs`, an area this slice did not
touch). The phrase "a hole in the quote" is a misleading description of the
symptom; what needs implementing is not on the quasiquote side but this
missing `collect` supply. With only 3 occurrences and a narrow blast radius, I
stopped at identifying the root this time.

slick: `errors=155 files_with_errors=52` to `errors=149 files_with_errors=49`.
