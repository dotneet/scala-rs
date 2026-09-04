## Architecture

The crates of the Cargo workspace:

| crate            | role                                                                                    |
| ---------------- | --------------------------------------------------------------------------------------- |
| `scala-rs-span`   | source positions and diagnostics                                                        |
| `scala-rs-lexer`  | lexing (newline tokens for semicolon inference, mode stack for `s`/`f`/`raw"..."`)       |
| `scala-rs-parser` | recursive-descent parser; the AST is close to nsc's `Tree`                               |
| `scala-rs-pickle` | reader for nsc `ScalaSignature` pickles; used by both `typer` and `backend`              |
| `scala-rs-typer`  | namer + typer + uncurry + lambda-lift + erasure, including implicit search               |
| `scala-rs-backend`| JVM class file emission (major 52 / `StackMapTable`) and the scala-rs runtime            |
| `scala-rs-driver` | drives the pipeline                                                                     |
| `scala-rs-cli`    | command line; binary `scala-rs`                                                         |

### Supplying symbols from `ScalaSignature`

For a long time the members of the standard library were hand-written in
`crates/typer/src/prelude*.rs`. That approach does not scale to 2.13
compatibility, so there is now a path that **reads the `ScalaSignature` (nsc
`PickleFormat`) embedded in scala-library's class files and supplies symbols
from it**. It coexists with the hand-written prelude and **fills in, on demand,
only the members the prelude does not have**.

| module                            | role                                                                          |
| --------------------------------- | ----------------------------------------------------------------------------- |
| `crates/pickle/src/codec.rs`      | SID-10 ByteCodecs (shared with the writer)                                    |
| `crates/pickle/src/classfile.rs`  | just enough class file parsing to reach `ScalaSignature`; also handles `ScalaLongSignature` (array-valued) |
| `crates/pickle/src/names.rs`      | Scala `NameTransformer` (`++` ↔ `$plus$plus`); shared with the backend         |
| `crates/pickle/src/read.rs`       | the pickle **reader**: bytes → entry table                                    |
| `crates/pickle/src/sym.rs`        | entry table → class signature; walks parents and substitutes type arguments   |
| `crates/typer/src/pickle_supply.rs` | `SigType` → `scala_rs_parser::Type`, and installation into the `SymbolTable` |
| `crates/backend/src/pickle.rs`    | the pickle **writer** (pre-existing; a subset of nsc's `PickleFormat`)         |

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

It is called **only when member resolution in `check.rs` has failed
completely**. Three rules keep it from lying.

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
   and its companion and merges the results. It used to be "if the class side
   supplied even one member, do not look at the companion", which made the
   answer **depend on unrelated global state**. `scala.math.BigDecimal` declares
   an instance method `apply(MathContext)` whose parameter type cannot be
   represented until `java.math.MathContext` is in the symbol table — so the
   class side only succeeded after something had touched `java.math.BigDecimal`,
   and the companion's seven `apply`s were then dropped wholesale, making
   `BigDecimal(2)` compile or not **depending on statement order**. Merging is
   order-independent.

When an overload set **spans several owners** (a class and its companion),
`resolve_overload` in `check.rs` re-derives the candidate symbols from the owner
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

#### How much of the hand-written prelude could be replaced (investigation only; nothing deleted)

Because the fill-in runs only when resolution fails, there is normally no way to
tell whether a member already in the prelude could have been built from a
pickle. So a temporary hook pointed `PickleSupply::complete` directly at members
the prelude already has, to print **what signatures the pickle alone can
produce**. Of the 39 hand-written members of `List` / `Option` / `Vector`, **38**
can be produced.

| receiver | producible from the pickle |
| -------- | -------------------------- |
| `List`   | `map` `foreach` `head` `tail` `isEmpty` `length` `size` `nonEmpty` `reverse` `apply` `contains` `exists` `forall` `toList` `toString` `collect` `zip` `sum` `min` `max` `indexOf` `drop` `take` |
| `Option` | `get` `isEmpty` `isDefined` `getOrElse` `map` `flatMap` `foreach` `filter` `toList` `orElse` |
| `Vector` | `map` `apply` `length` `foreach` `head` |

The only one that could not be produced is `List#withFilter` (its `WithFilter`
return type is a class the prelude holds in a shape of its own, which does not
line up with the pickle's).

**But this only says the shape of the signature can be obtained; it is not
grounds for deleting anything as it stands.** In fact `List#zip` comes out as
`List[(tparam#289, tparam#2739)]` and `Option#orElse` as
`(=> #29[tparam#2719])Option[B]`: the type parameter bindings are broken in the
rendering. Replacing them would have to be done one at a time, checked against
the fixtures' actual output. This round **only leaves the list and the evidence;
nothing was removed from the prelude**.

To reproduce, write a temporary test that calls `PickleSupply::complete`
directly on already-preluded symbols (it rebuilds the symbol table per member,
so 39 of them take about 100 seconds).

#### The codegen side

**No change to `gen.rs` was needed.** The existing machinery lines up as is.

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
  registered keeps the source name. `NameTransformer` was moved to
  `crates/pickle/src/names.rs` and is shared with the backend (the assembler
  already encoded output names, so codegen needed no change).
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

#### What works today

With `--scala-library <jar>`, the following typecheck **without a single line in
the prelude** and produce output **byte-identical** to scalac 2.13.16's under
`java -Xverify:all -cp out:jar Main`.

- `List`: `filter` `filterNot` `count` `exists` `forall` `take` `drop`
  `takeWhile` `dropWhile` `reverse` `mkString` (0/1/3 args) `contains` `indexOf`
  `init` `last` `distinct` `startsWith` `splitAt` `partition` `span` `slice`
  `headOption` `lastOption` `find` `sorted` `sortBy` `sortWith` `max` `min`
  `maxBy` `toVector` `toSet` `toSeq` `toArray` `scanLeft` `zip` `padTo`
  `updated` `patch` `indexWhere` `tails` `combinations` `permutations`
  `zipWithIndex` `grouped` `sliding` (1/2 args) `view` `iterator` `flatMap`
  `foldRight` `reduce` `reduceLeft` `copyToArray` `sum` `product`
- Operators: `:+` `+:` `++` `++:`; `&` `|` `&~` `++` on `Set`; `+` `-` on `Map`
- `Map`: `map` `filter` `keySet`. `Set`: `map` `filter`. `Vector`: `map`
  `filter` `mkString`
- `Range` / `IndexedSeq`: `filter` `map`
- `Option`: `exists` `forall` `contains` `filter` `toList`
- Companions: `Iterator.from` `.continually` `.single`, `List.fill` `.tabulate`,
  `Vector.fill` `.tabulate`, `Set.empty`

There are two places where the treatment of type parameters deliberately matches
nsc's.

- `scala.package.List` / `scala.package.Ordering` are **type aliases** in the
  package object, and the pickle refers to them by the alias name. Rather than
  keeping a table, the `ALIASsym` is looked up in `scala/package.class`'s pickle
  and expanded. For the path where the source uses the same alias **by name**
  (`new NoSuchElementException("x")` / `Ref[F, A]`), see the section "type
  aliases in a jar's package object": the same `ALIASsym` is registered as a
  type member of the package instead of being expanded.
- `def max[B >: A](implicit ord: Ordering[B]): A` gives the call site nothing to
  determine `B` from. scalac resolves it to the lower bound `A`, so we do the
  same. Without it the typer could not solve `Ordering[B]` and — instead of
  reporting an error — **eta-expanded `xs.max` into a function value** and
  printed `Main$$$anonfun$4@...`. Members that still have undetermined type
  parameters after this step are not supplied.

#### What does not work yet

- **Rebuilding classes that are already in the symbol table.**
  `scala/collection/Seq` is installed by `find_or_stub_java_class` **without type
  parameters**, so `Seq[B]` does not match and `diff` / `intersect` / `union` /
  `indexOfSlice` / `containsSlice` cannot be supplied. Retrofitting the pickle's
  type parameters did work once, but reshaping symbols the prelude built has wide
  effects: the moment `Seq` changed, the **hand-written** `segmentLength` /
  `scanRight` stopped resolving. Breaking what works is worse, so the table is
  left alone. Stubbing a class that is not in the table remains fine.
- **Stubs get no parents** (when creating a class that is not in the table).
  Giving them a parent chain changes subtyping globally, so they get only
  `Type::AnyRef`. A stub type is essentially usable only as itself. Note that
  classes that were *completed* do get the parents the pickle declares
  (`attach_parents`); without that, `Set#&` (whose argument is
  `collection.Set[A]`) could be supplied but not called. The 11th slice widened
  this in exactly two ways. (a) An **empty placeholder** installed by
  `find_or_stub_java_class` gets the pickle's type parameters even under a
  `scala/` name, as long as it was allocated after `prelude_end`
  (`give_stub_its_kinds`; symbols the prelude built are still untouched). (b)
  When a parent denoting the same class is already present but **differs only in
  its arguments**, the pickle's version refines it — a class file's generic
  signature can only say `ReusableBuilder<T, Object>`, and since `To` is
  invariant, `ArrayBuilder[E]` would not become `Builder[E, Array[E]]`.
- **A mismatch in the default-getter convention.** `default_getter_apply` passes
  the actual arguments preceding the default to the getter, whereas scalac emits
  `SeqOps.lastIndexOf$default$2()` with no arguments. Shapes that disagree are
  not supplied (`lastIndexOf` currently falls out here). Fixing it means touching
  the default-argument path in `check.rs`.
- **`String.format`**: it goes through the `augmentString` → `StringOps`
  **extension method** path, and the fill-in hook sits after member resolution
  fails. The receiver is `java/lang/String`, so it does not land in `scala/`
  scope either.
- **`scala.io.Source`**: resolved on the Java class file loader side, not through
  the pickle path.
- **`reduceOption`**: `[B >: A](op: (B, B) => B): Option[B]`. `B` cannot be
  solved from the lambda, and `bound_lo` does not reach it (an inference-side
  matter).
- **`collect { case … }`**: inferring from an inline partial function literal is
  a typer limitation that predates pickle supply (`list_collect.scala` passes a
  named `PartialFunction`).

`SCALA_RS_PICKLE_DEBUG=1` traces which members were supplied, and why the others
were not.

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
  This table was initially off by one from bit 16 up, so `is_public_api` was
  reading STABLE where it meant SYNTHETIC and JAVA where it meant LOCAL (the
  error was in the over-rejecting direction, so the results happened to come out
  right). It is now pinned across every position by
  `flag_bits_match_the_library`, against real symbols. Bits 30 and above are not
  named because there has been no need.
