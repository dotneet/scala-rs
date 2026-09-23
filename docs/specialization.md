# `@specialized`

**Status: method-owned `Int`/`Long` specialization is implemented; class- and
trait-owned specialization is not.** No `Foo$mcI$sp` class is emitted, so a
class we compile with a specialized type parameter is not ABI-compatible with
what scalac produces for the same source. `tests/spec_classfiles.sh` measures
that gap (the current figures are in `tests/BASELINE.md`).

`@specialized` is a performance annotation: a program compiled with and without
specialization computes the same answers. What changes is boxing and the set of
classes and methods on disk, which is what a separately compiled consumer links
against. The rule this compiler follows is therefore: **never advertise a
specialized entry that is not emitted.**

## What is implemented

### Accepting and recording the annotation

- `crates/parser/src/specialization.rs` reads an annotation tree into a
  `SpecializedTypes` set, following nsc's `SpecializeTypes.specializedOn`: no
  arguments means `Specializable.Primitives` (the nine primitive value classes,
  not `AnyRef`), a group name expands to its members as
  `scala/Specializable.scala` spells them, anything else is a type name, and a
  name that is neither selects nothing. `SpecializedType::tag` carries the
  `$mc<letter>$sp` letters.
- `crates/parser/src/parse.rs` (`parse_annotation`) keeps the annotation and
  normalises its spelling, including `import scala.{specialized => sp}`.
- `crates/typer/src/symbol.rs`: `Symbol::specialized` (on the type parameter)
  and `Symbol::unspecialized` (on the member), filled by
  `SymbolTable::record_specialization`.
- `-no-specialization` drops the annotation, as in nsc.
- The phase's one default warning, `type A is unused or used in
  non-specializable positions.` (nsc's `normalizeMember`), is issued by
  `warn_refchecks.rs`.

`import scala.{specialized => sp}` and `import scala.Specializable._` name terms
the private runtime (`--no-scala-library`) does not define, so those imports
compile only against the real jar; the annotation itself is accepted in both
modes.

### The method slice (`crates/typer/src/specialize.rs`)

One method-owned type parameter, the `Int` and `Long` selections, on module
methods and methods that are `final` or `private`:

- The generic method, its descriptor, body and source pickle entry are kept.
- A sibling with nsc's exact name (`id$mIc$sp(I)I`, `id$mJc$sp(J)J`) is added in
  the same owner. The selected primitive is substituted through parameter
  types, result type and the body (including local classes, constructors and
  type symbols, which are cloned per entry) before erasure.
- A call whose selected type argument is statically `Int` or `Long` is
  rewritten to the sibling; `String`, type-variable and unsupported selections
  stay on the generic entry.
- `@unspecialized` suppresses a member's variants.
- The driver runs the pass after `pickle_all` and before `erase`: the generic
  declaration is pickled first, the variants are JVM-only members. The source
  pickle keeps the method type parameter's `@specialized` annotation and nsc's
  `SPECIALIZED` flag, but only for selections that were actually emitted — a
  scalac consumer selects `$mIc$sp` from that metadata, not from the method
  name. A provider with a method *named* `id$mIc$sp` but without the metadata
  makes scalac box and call `id(Object)Object`.

Everything outside the slice (other primitives, `AnyRef`, several type
parameters, override-capable class methods, class and trait type parameters)
keeps the generic behaviour and publishes no specialized metadata. It is not
rejected: the program is legal, it is just not specialized.

Tests: `crates/cli/tests/specialized.rs` (ABI and bodies, local classes,
constructors and captures, bounds, and scalac consumers of our output),
`crates/typer/tests/specialization.rs`, and the `sp_*` fixtures.

### Consuming scalac's specialized classes

The reading side works independently of the above:

- `PickleSupply` maps a pickled parent `Foo$mcI$sp` back to `Foo`
  (`despecialized()` in `crates/typer/src/pickle_supply.rs`).
- A binary specialized method that returns a primitive is not unboxed a second
  time.
- A scalac-built specialized pair such as `Tuple2$mcII$sp` (what a case class
  `unapply` over primitives returns) stores its values in `_1$mcI$sp` and never
  writes the generic `_1` / `_2` fields, so under the library ABI tuple
  components are read through the `_1()` / `_2()` accessors, as nsc does
  (`crates/cli/tests/specialized_tuple_fields.rs`). The private runtime's
  `Tuple2` keeps its field reads.

## What is not implemented: the class and trait ABI

For `class Box[@specialized(Int) A](var value: A) { def get: A; def set(v: A) }`,
scalac 2.13.16 (JDK 17) emits:

- On the generic `Box<A>`: the source field and methods
  (`value()Ljava/lang/Object;`, `get`, `set`, `<init>(Object)`), plus a primitive
  dispatch surface — `value$mcI$sp()I`, `value$mcI$sp_$eq(I)V`,
  `get$mcI$sp()I`, `set$mcI$sp(I)V` — whose bodies box/unbox through the
  generic storage, and `specInstance$()Z` returning `false`.
- A public class `Box$mcI$sp extends Box<Object>` with its own primitive field
  `value$mcI$sp:I`, a primitive constructor `<init>(I)V`, primitive members,
  erased bridges that box at the boundary, and `specInstance$` returning `true`.
  Only `Box` carries a `ScalaSignature`; the sibling has a JVM `Signature` of
  `LBox<Ljava/lang/Object;>;` and no pickle of its own.
- `new Box[Int](1)` becomes `new Box$mcI$sp(I)`; `new Box[String]` stays
  generic. A consumer whose static type is `Box[Int]` calls the primitive
  methods on the generic owner (`Box.get$mcI$sp`), so dispatch reaches the
  specialized subclass. `class IntBox extends Box[Int]` extends `Box$mcI$sp`.
- For an array-shaped field (`var value: Array[A]`) the generic field stays
  `Object` while the sibling has `value$mcI$sp:[I` and `<init>([I)V`: source
  types and JVM descriptors have to be carried separately.
- Traits get `T$mcI$sp` marker interfaces, specialized default/static helpers
  and implementation bridges.

This is also a metadata-integrity constraint: copying only `Box.class` (with
its pickle still advertising `@specialized(Int)`) lets scalac compile a
consumer that then fails with `NoClassDefFoundError: Box$mcI$sp`. A producer
must publish the specialized class, its constructor and field, the
generic-owner primitive declarations and all bridges as one unit. Until then,
`crates/backend/src/pickle.rs` drops class and trait `@specialized`
annotations from the pickle rather than promise entries that do not exist.

The work splits into: (1) a class-variant record per selection, kept beside
the generic class record; (2) the generic-owner dispatch surface; (3)
materialising the sibling class with primitive fields, constructors, members
and `ACC_BRIDGE | ACC_SYNTHETIC` bridges; (4) rewriting `new` and direct
subclasses; (5) a separate-compilation gate — a scalac consumer against our
classes, `javap -c -p -s`, and `java -Xverify:all`. Traits, override dispatch
and the remaining primitive tags come after that. Generated classes have to
take part in the JVM name index and classpath metadata, so `Gen::extras` alone
is not enough.

The ledger `tests/spec_classfiles.sh` compiles each `test/files/pos/spec-*.scala`
of the corpus with scalac and with us and diffs the *sets of classfile names*.
It compares names only; it does not verify descriptors or run anything, so a
matching name is not the acceptance condition for the class phase.

Related `neg` tests whose expected error only the phase produces, and which
are therefore accepted today: `spec-overrides`, `t4417`, `t4541`, `t5564`,
`t9014`.
