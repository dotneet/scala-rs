## Architecture

The crates of the Cargo workspace:

| crate            | role                                                                                    |
| ---------------- | --------------------------------------------------------------------------------------- |
| `scala-rs-span`   | source positions and diagnostics                                                        |
| `scala-rs-lexer`  | lexing (newline tokens for semicolon inference, mode stack for `s`/`f`/`raw"..."`)       |
| `scala-rs-parser` | recursive-descent parser; the AST is close to nsc's `Tree`                               |
| `scala-rs-pickle` | reader for nsc `ScalaSignature` pickles; used by both `typer` and `backend`              |
| `scala-rs-typer`  | namer + typer (including implicit search and macro expansion) and the tree lowerings up to erasure |
| `scala-rs-backend`| JVM class file emission (major 52 / `StackMapTable`), the pickle writer, and the private runtime |
| `scala-rs-driver` | drives the pipeline                                                                     |
| `scala-rs-cli`    | command line; binary `scala-rs`                                                         |

### Pipeline

`crates/driver/src/lib.rs::compile_paths` runs, for the whole compilation run
against one symbol table:

1. **parse** every unit (`crates/parser`);
2. **namer and typer** (`typecheck_units_src`): the namer enters every unit,
   then a header pass types all class parents, a signature pass all member
   signatures, and a body pass the bodies. Members without a type annotation
   are completed lazily (`lazysig.rs`). Constant folding of `final val`s
   (`const_fold.rs`), macro expansion, quasiquote/`reify` lowering and the
   `-Xasync` transform happen here;
3. local-object checks, then per unit: default-receiver hoisting, restoring
   named-argument and right-associative evaluation order, **uncurry**, local
   `lazy val` lowering, **lambda-lift**, capture analysis for local and
   anonymous classes (`anon_capture.rs`), and private-name expansion;
4. value-class companions, override families and super accessors are
   recorded; the **pickler** (`backend::pickle::pickle_all`) writes every
   `ScalaSignature` from the typed symbols;
5. the **method specialization** pass (`specialize.rs`, see
   [specialization.md](specialization.md)), which runs after the pickler as
   nsc's does;
6. generic `Signature` attributes are recorded (`backend/src/sig.rs`), then
   **erasure** (`erasure.rs`) rewrites types, inserts boxing and marks bridges;
7. **code generation** (`crates/backend`), including tail-call loops
   (`gen_tailrec.rs`), lambdas as `invokedynamic`, and bridges.

### Supplying symbols from `ScalaSignature`

The standard library is described by two sources: a hand-written prelude
(`crates/typer/src/prelude*.rs`) and the **`ScalaSignature` (nsc
`PickleFormat`) embedded in scala-library's class files**. The pickle path
coexists with the prelude and **fills in, on demand, only the members the
prelude does not have**. Scala classes from other jars and class directories
on `-cp` are read the same way: `PickleSupply::adopt_binary_class` overlays a
class-file symbol with its pickle (type-parameter kinds such as `F[_]`, which a
JVM generic signature cannot express, and each member's Scala signature),
keeping the class-file description for anything the pickle cannot express.
`java.*` classes and the symbols the prelude built are not adopted.

| module                            | role                                                                          |
| --------------------------------- | ----------------------------------------------------------------------------- |
| `crates/pickle/src/abi.rs`        | binary names and flags shared at the class-file/pickle boundary               |
| `crates/pickle/src/codec.rs`      | SID-10 ByteCodecs (shared with the writer)                                    |
| `crates/pickle/src/classfile.rs`  | just enough class file parsing to reach `ScalaSignature`; also handles `ScalaLongSignature` (array-valued) |
| `crates/pickle/src/names.rs`      | Scala `NameTransformer` (`++` ↔ `$plus$plus`); shared with the backend         |
| `crates/pickle/src/read.rs`       | the pickle **reader**: bytes → entry table                                    |
| `crates/pickle/src/sym.rs`        | entry table → class signature; walks parents and substitutes type arguments   |
| `crates/typer/src/pickle_supply.rs` | `SigType` → `scala_rs_parser::Type`, and installation into the `SymbolTable` |
| `crates/backend/src/pickle.rs`    | the pickle **writer** (a subset of nsc's `PickleFormat`)                       |

`crates/pickle` is its own crate because `crates/typer` cannot depend on
`crates/backend` (the dependency runs the other way).

#### The reader

`read.rs` handles **all** the tags of nsc 2.13's `PickleFormat.scala` (symbols,
types, literals, `SYMANNOT`, `ANNOTINFO`, `CHILDREN`, the `TREE` variants,
`MODIFIERS`). As a matter of policy, **an unknown tag or a body whose length
does not add up becomes a `ReadError` rather than being swallowed**. Every entry
is verified to consume exactly the length it declared, so misreading the format
turns straight into a failing test.

`sym.rs` opens the parent classes' class files on demand through `ClassSource`
and **substitutes the parent's type arguments at every hop**, so the answer
comes back in the vocabulary of the class that was asked about.

```
List#filter (from scala.collection.IterableOps)
    (pred: scala.Function1[A, scala.Boolean])scala.collection.immutable.List[A]
```

`IterableOps` declares a return of the opaque `C`; substitution turns it into
`List[A]`. Without that the typer cannot bind `C`.

#### Hooking into type checking (`pickle_supply.rs`)

It is called **only when member resolution in the typer (`check_*.rs`) has
failed completely**. Four rules keep it from lying.

1. **The hand-written prelude always wins.** It runs only after nothing was
   found, so it never overrides or shadows an existing declaration (pinned by
   `the_prelude_wins_over_the_pickle`).
2. **Members that cannot be represented faithfully are not supplied.** If the
   type does not fit into `scala_rs_parser::Type`, or the erased descriptor is
   not uniquely determined, nothing is supplied and the usual `is not a member`
   comes out. No type is better than a wrong type.
3. **No prefetching.** One class file per failed `(receiver, name)` pair, cached
   afterwards.
4. **Look at both the class side and the companion side, and merge them.** When
   the receiver is a class, `PickleSupply::complete` queries **both** the class
   and its companion and merges the results. Answering from whichever side
   supplied something first made the result depend on unrelated global state:
   `scala.math.BigDecimal`'s instance `apply(MathContext)` is representable
   only once `java.math.MathContext` is in the symbol table, and when it won,
   the companion's seven `apply`s were dropped, so `BigDecimal(2)` compiled or
   not depending on statement order.

Two other callers ask the pickle without a failed lookup: a class file that
declares a signature none of the hand-written candidates has
(`Check::supply_receiver_override`), and a source class with a generic
`scala.*` ancestor, whose overridden members are completed before the bridge
pass so that erasure bridges exist (for example `Numeric.fromInt` for
`class Num extends Numeric[Int]`).

When an overload set **spans several owners** (a class and its companion),
`resolve_overload` (`check_overload.rs`) re-derives the candidate symbols from the owner
of `fun.sym`, because `Type::Overload` carries only types. That drops one
owner's candidates **entirely**, so the sets lost in the re-derivation are now
remembered in `Check::overload_groups` and used. On top of that, and only when
no argument matched at all, a **selection that used a class name in term
position** (which in nsc denotes the companion object) is widened with the
companion's members and resolved once more (`Check::widen_with_companion`). It
sits only on the path immediately before reporting an error, so it can only turn
a rejection into a resolution.

Erased descriptors are not obtained by reimplementing scalac's erasure; they are
taken **from the class file's method table itself** (walking supers and
interfaces — `List#mkString` is a default method on `IterableOnceOps`). When two
candidates of the same arity exist in the same hierarchy, no choice is made and
the member is not supplied.

`SCALA_RS_PICKLE_DEBUG=1` traces which members were supplied, and why the others
were not.

#### The codegen side

- When a method symbol's `jvm_name` starts with `(`, `method_desc_from_sym`
  uses it directly as the descriptor. Supplied members put the erased descriptor
  there.
- A call's owner is the symbol's owner, that is **the receiver class itself**, so
  `invokevirtual scala/collection/immutable/List.mkString(...)` resolves
  correctly for both inherited methods and interface default methods.
- The checkcast / unbox for an `Object` result is already handled by
  `maybe_unbox_erased_result`.

#### Search order is linearization (SLS 5.1.2)

Which type-argument binding an inherited member comes back with is decided by
**the order in which parents are searched**. `immutable.Set[A]` mixes in
`Iterable[A]` and then `SetOps[A, Set, Set[A]]`, so by SLS 5.1.2's "later
parents win" rule, `IterableOps`'s opaque `C` resolves to `Set[A]`. Breadth-first
would reach `IterableOps` through `Iterable` first and return the weaker
`Iterable[A]`, a type that is not in the symbol table — at which point the member
was given up on entirely.

`L(C) = C, L(Cn) +: … +: L(C1)` is folded from the left as
`acc = L(Ci) ++ (acc − L(Ci))`. The collection hierarchy is wide, so there are
caps on depth and on total steps.

#### Names, overloads and default arguments

- **Operator names**: nsc keeps operator names **encoded**. `SetOps` pickles `&`
  as `$amp`, and the class file declares `$amp` too. So both the pickle lookup
  and the descriptor lookup are done with the **encoded** name, while the symbol
  registered keeps the source name. `NameTransformer` lives in
  `crates/pickle/src/names.rs` and is shared with the backend.
- **Overload deduplication** is done on the erased parameter list. Declarations
  that erase to the same thing are the same JVM method seen through different
  parents; when they differ only in result type
  (`IterableOps.map[B]: Iterable[B]` versus `MapOps.map[K2,V2]: Map[K2,V2]`),
  scalac picks by expected type and we cannot, so we take the more derived one,
  the one that comes first in linearization order. Ones that differ in
  parameters (`Iterator.from(Int)` and `from(IterableOnce)`) are different
  methods and both are kept.
- However, **only one overload that takes a function** is kept. A lambda's
  parameter types can only be inferred from a single expected type, so adding a
  second one makes `xs.segmentLength(_ < 3)` an unsolvable overload set.
- **Default arguments**: the parameters are marked and the class's
  `<method>$default$<n>` getters are supplied along with the member (the getters
  are synthetic, so the filter is relaxed only when they are being fetched on
  purpose). A member whose getters cannot be supplied is **not supplied at all**.
  Without that, `xs.lastIndexOf(2)` typechecks and then emits bytecode that calls
  a two-argument descriptor with one argument, giving a `VerifyError`.

#### Type parameters that match nsc

- `scala.package.List` / `scala.package.Ordering` are **type aliases** in the
  package object, and the pickle refers to them by the alias name. The
  `ALIASsym` is looked up in `scala/package.class`'s pickle and expanded.
  Where the source uses such an alias **by name** (`new
  NoSuchElementException("x")`, `Ref[F, A]`), the same `ALIASsym` is
  registered as a type member of the package instead.
- `def max[B >: A](implicit ord: Ordering[B]): A` gives the call site nothing
  to determine `B` from. scalac resolves it to the lower bound `A`, and so do
  we. Members that still have undetermined type parameters after this step
  are not supplied.

#### Limits

- **Symbols the prelude built are not reshaped.** Retrofitting pickle type
  parameters or parents onto a hand-written class changes what the other
  hand-written members resolve to, so the prelude always wins
  (see [prelude-fidelity.md](prelude-fidelity.md) for what that costs).
- **Stubs get no parents.** A class created only as a placeholder gets
  `Type::AnyRef` as its sole parent, because a parent chain would change
  subtyping globally. Classes that are *completed* get the parents the pickle
  declares (`attach_parents`); an empty placeholder allocated after
  `prelude_end` gets the pickle's type parameters (`give_stub_its_kinds`); and
  a parent that differs only in its arguments is refined from the pickle
  (a class file's generic signature can only say `ReusableBuilder<T, Object>`,
  so `ArrayBuilder[E]` would not otherwise be a `Builder[E, Array[E]]`).
- Classes from plain Java (and the Java-written parts of the library) are
  read from their class files, not through this path.

### What the 2.13.16 pickles revealed

- `List$.class` has **no** `ScalaSignature`. A companion pair's pickle is stored
  only in the class-side class file, so a module class falls back to its
  companion.
- Classes that come from plain Java (`BoxesRunTime` / `*Ref` / `ScalaNumber` /
  the node classes in `scala.collection.concurrent`) have no `ScalaSignature`.
- `pflags` (the flag bits in a pickle) do not sit where the raw `Flags` do,
  because nsc permutes the low 12 bits through `rawToPickledFlags`. Bit 12 and
  above match raw, and **some bits are shared between terms and types**
  (`COVARIANT`/`BYNAMEPARAM` are the same bit; so are `TRAIT`/`DEFAULTPARAM`).
  The table is pinned across every position by `flag_bits_match_the_library`
  (`crates/pickle/tests/lib_jar.rs`), against real symbols. Bits 30 and above
  are not named.
