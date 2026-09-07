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

The third row is the one that used to be missing. A class file records no
per-parameter bit at all, so a member the pickle path declines is described by
`classpath::fill_java_members` alone, and that reader ignored the getters
sitting next to the method. json4s's

```scala
FieldSerializer[String]()
```

reported

```
no matching overload for
  (PartialFunction[…], PartialFunction[…], Boolean, ClassTag[String])FieldSerializer[String]
  with arguments ()
```

The getter's name does not say which *overload* it belongs to.
`org.scalatra.Control` declares `halt(ActionResult)` next to
`halt[T](Integer = null, T = (), Map = Map.empty)`; arity settles most slots and
the getter's result descriptor settles the rest
(`classpath::getter_fills_param`).

## A case class's companion `apply` is `SYNTHETIC`

`Member::is_public_api` filters `SYNTHETIC` out along with bridges and
`$anonfun`s, so the pickled `apply`/`unapply`/`copy` of a `case class` were
never installed and the class file's cruder description stood in their place --
one that records neither the defaults nor which clause is `implicit`.
`Member::is_case_synthetic` (`CASE` alongside `SYNTHETIC`, which nsc sets on
exactly the members it derives from a `case` declaration) admits those three,
and `adopt_binary_class` drops the class file copies it replaces.

## Which prefix the getter is selected off

The getter is a fresh `Select` built by `Typer::default_getter_apply`, so it
needs a receiver even though the source wrote none. Taking `this` is right only
when the enclosing class really has the member:

* a name that arrived through `import <object>._` belongs to the object --
  "value avatar$default$3 is not a member of IndexControllerBase";
* a name a cake's self type contributes, seen from inside an anonymous class,
  belongs to the class that carries the annotation and is reached through that
  class's `$outer` -- "value getAccountByUserNameIgnoreCase$default$2 is not a
  member of $anon$61";
* a `def` written inside a method body has its getter in that same body. There
  is no receiver and no class to emit it on, so the default's own expression is
  spliced instead, in the scope that wrote it. twirl writes gitbucket's
  templates as a local `def menuitem(…, count: Int = 0)` inside `apply`.

`Typer::default_getter_receiver` decides between those.

### The second shape was also a miscompilation

The *main* call in the anonymous-class case compiled to `aload_0; checkcast
AccountService` on an object that does not implement it -- a
`ClassCastException` from a program that type-checked, with explicit arguments
and no defaults involved. `gen_desc::outer_self_type_reaches` teaches the
backend's receiver walk that an enclosing class's **self type** supplies
members too, so the `$outer` is loaded and cast instead.
`gen_desc::self_type_supplies` is the matching stopping condition inside
`load_owner_instance`: the class that carries the annotation really is mixed
with its self type at run time, so the walk ends there rather than running on
to the outermost enclosing instance.

## A compound self type contributed only its first component

`SymbolTable::class_sym_of` answers a `Type::Refined` with its first parent, so
`self: WikiService & RepositoryService & AccountService & … =>` (six
components, which is how every gitbucket controller is written) made only
`WikiService`'s members visible from inside. `SymbolTable::self_type_classes`
returns all of them, and the three traversals that walk a self type
(`lookup_member`, `members_including_inherited`, `is_ancestor_of`) use it.

## Type parameters and defaults

Two places have to *withhold* the parameter's declared type rather than demand
conformance to it, because the default is what determines the type argument:

* **the call site.** `halt(400)` on
  `def halt[T: ClassTag](status: Int = 400, body: T = (), …)` fills `body` from
  `halt$default$2()`, whose result is `Unit`. Checking that against the
  unsolved `T` reported "type mismatch; found: Unit  required: T".
  `default_getter_apply` types the getter call with no expectation when the
  parameter's type still mentions a type parameter, which is what
  `pretype_spliced_default` already did for a spliced default.
* **the supply of the getter itself.** nsc infers a default getter's result
  type, so `halt$default$1` is `[T]()Integer` -- a type parameter the signature
  mentions nowhere else. `pin_undetermined_tparams` refused such a shape, and a
  getter that cannot be supplied makes the *method* ineligible, which is how
  `halt(400)` lost its only applicable overload. A type parameter no parameter
  and no result names is now kept: nothing at the call site depends on how it
  is solved.

## Not implemented

* **Declaring** `def f[T](x: T = ())` in source. scala-rs types the default
  against the parameter's declared `T` and reports a mismatch; nsc infers the
  getter's result type instead and accepts it. Reading such a method back out
  of a class file works (`tests/multi/defaultargs_binary/Halt_1.scala`), which
  is what the libraries need; writing one does not.
* **Curried defaults in a class file scala-rs itself produced.** A `-cp` class
  compiled by scala-rs is described by its class file only -- the typer reads
  pickles from the standard library and from classes `adopt_binary_class` has
  taken over, not from arbitrary `-cp` output -- and a class file flattens the
  clauses, so `def join(a: String)(b: String = "-")(c: String = a + b)` comes
  back as a single three-parameter method. Against **nsc's** class files the
  pickle is read and the same declaration works.

## Tests

* `tests/fixtures/da_defaults.scala` (+ `_bad`): every shape above in source,
  run in both the private-runtime and `--scala-library` modes, printing what
  each default produced. Output verified against scalac 2.13.16.
* `tests/multi/defaultargs_binary/`: `dalib` compiled by **real scalac**, the
  consumer by scala-rs. This is the only setting the class-file root appears
  in.
* `crates/cli/tests/defaultargs.rs` drives both.
