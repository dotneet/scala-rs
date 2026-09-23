# Default arguments

A default argument is a **call**, not a stored constant. nsc gives every
defaulted parameter a getter beside the method (`f$default$2`), and a call
site that omits the argument calls that getter, in argument order, passing the
arguments of the parameter clauses that precede it. Filling the slot with
anything else -- a re-typed copy of the declaration's expression, a zero value,
a guess -- produces a program that compiles and computes the wrong thing.

This note records where the getter comes from, which prefix it is selected
off, and what is still missing.

## Where the "this parameter has a default" bit comes from

| the method is declared in | what says the parameter has a default |
|---|---|
| the source being compiled | the parser's `Flags::DEFAULTPARAM` on the parameter |
| a Scala class file, via its pickle | `pflags::DEFAULTPARAM` on the pickled parameter (`pickle_supply::install`) |
| a class file whose pickle is not used | **the `name$default$n` methods beside it** (`classpath::mark_defaults_from_getters`) |

A class file records no per-parameter bit, so for a member the pickle path
declines, the getters sitting next to the method are the only evidence. The
getter's name does not say which *overload* it belongs to (scalatra's
`Control` declares `halt(ActionResult)` next to
`halt[T](Integer = null, T = (), Map = Map.empty)`); arity settles most slots
and the getter's result descriptor settles the rest
(`classpath::getter_fills_param`).

## A case class's companion `apply` is `SYNTHETIC`

`Member::is_public_api` filters `SYNTHETIC` out along with bridges and
`$anonfun`s, which would leave the pickled `apply` / `unapply` / `copy` of a
binary `case class` uninstalled and the class file's cruder description --
no defaults, no `implicit` clause -- in their place.
`Member::is_case_synthetic` (`CASE` alongside `SYNTHETIC`, which nsc sets on
exactly the members it derives from a `case` declaration) admits those three,
and `adopt_binary_class` drops the class-file copies it replaces.

This is **not** done for prelude classes: `scala.*` classes the prelude built
are authoritative, and `adopt_binary_class` refuses them. Offering the
pickled `Some$.apply` there made `Some("first")("spurious")` compile as
`Some$.apply("spurious")` (scala/scala's `neg/t4196`; the shape is in
`tests/fixtures/da_defaults_bad.scala`).

## Which prefix the getter is selected off

The getter is a fresh `Select` built by `Typer::default_getter_apply`, so it
needs a receiver even though the source wrote none. `this` is right only when
the enclosing class really has the member; `Typer::default_getter_receiver`
decides:

* a name that arrived through `import <object>._` belongs to the object;
* a name a self type contributes, seen from inside an anonymous or nested
  class, belongs to the class that carries the self-type annotation and is
  reached through `$outer`;
* a `def` written inside a method body has its getter in that same body.
  There is no receiver and no class to emit it on, so the default's own
  expression is spliced instead, in the scope that wrote it (twirl templates
  write local `def`s with defaults).

On the backend side, `gen_desc::outer_self_type_reaches` lets the receiver
walk treat an enclosing class's **self type** as a supplier of members, so the
`$outer` is loaded and cast rather than `this`, and
`gen_desc::self_type_supplies` stops the walk at the class that carries the
annotation. A compound self type (`self: A & B & C =>`) contributes all of
its components (`SymbolTable::self_type_classes`), not just the first.

## Type parameters and defaults

Two places *withhold* the parameter's declared type rather than demand
conformance to it, because the default is what determines the type argument:

* **the call site.** `halt(400)` on
  `def halt[T: ClassTag](status: Int = 400, body: T = (), …)` fills `body` from
  `halt$default$2()`, whose result is `Unit`. `default_getter_apply` types the
  getter call with no expectation when the parameter's type still mentions a
  type parameter, as `pretype_spliced_default` does for a spliced default.
* **the supply of the getter itself.** nsc infers a default getter's result
  type, so `halt$default$1` is `[T]()Integer` -- a type parameter the
  signature mentions nowhere else. A type parameter no parameter and no result
  names is kept when the getter is supplied, because nothing at the call site
  depends on how it is solved; refusing it would make the *method* ineligible.

## Not implemented

* **Declaring** `def f[T](x: T = ())` in source. scala-rs types the default
  against the parameter's declared `T` and reports a mismatch; nsc infers the
  getter's result type instead and accepts it. Reading such a method back out
  of a class file works (`tests/multi/defaultargs_binary/Halt_1.scala`), which
  is what the libraries need.
* **Solving a class type parameter from an omitted constructor default**
  (`case class C[+F <: Option[Int]](n: String, f: F = None)` called as
  `C("q")`): the getter is emitted with nsc's descriptor, but the call site
  does not consult its result type.
* **Getters of defaults in a later parameter clause, as scalac reads them.**
  For `def join(a: String)(b: String = "-")(c: String = a + b)` we pickle
  `join$default$3` with one flat clause `(a, b)` where nsc writes `(a)(b)`, so
  a scalac client of our class file cannot omit those arguments
  (`not enough arguments for method join$default$3`). A scala-rs client of the
  same class files, and scala-rs against nsc's class files, both work.

## Tests

* `tests/fixtures/da_defaults.scala` (+ `_bad`): every shape above in source,
  run in both the private-runtime and `--scala-library` modes, printing what
  each default produced. Output verified against scalac 2.13.16.
* `tests/multi/defaultargs_binary/`: `dalib` compiled by **real scalac**, the
  consumer by scala-rs. This is the only setting the class-file row of the
  table above appears in.
* `crates/cli/tests/defaultargs.rs` drives both.
