# What the hand-written prelude drops that the pickle carries

`crates/typer/src/prelude*.rs` declares about 4,700 symbols by hand. A member
`PickleSupply` installs from a `ScalaSignature` arrives with the flags nsc
pickled, a parameter symbol per parameter, and its constructor fields; a member
`prelude::class` / `prelude::method` builds arrives with `Flags::FINAL` and
nothing else.

That would not matter if the pickle won. It does not: member lookup finds the
hand-written member first, and `Check::supply_from_pickle` runs only when
nothing matched, or when the *classfile* declares a signature none of the
candidates has (`Check::supply_receiver_override`). So an attribute the prelude
leaves off is an attribute the compiler does not have — in `--scala-library`
mode as much as in the private-runtime one.

Two instances of this were found separately in wave 10 (`TupleN` without
`Flags::CASE`, 53 errors; methods without parameter symbols, 3 in cats). This
is the survey that looks for the rest.

## How the survey was done

A throwaway test in the typer crate installed the prelude, then for every
prelude class whose JVM name starts with `scala/` loaded the same class's
`ClassSig` from `/tmp/scala-rs-lib/scala-library-2.13.16.jar` through
`SigLoader` and compared: `CASE`, `SEALED`, `ABSTRACT`, `TRAIT`/`INTERFACE`,
`FINAL`, type-parameter count, and per-member `IMPLICIT`, parameter-symbol
count, by-name and defaulted parameters. 546 differences over 222 classes.

Reproducing it is a ~150-line `#[cfg(test)]` module; it is not kept in the tree
because it asserts nothing — it prints a list. The list is below.

## The list, by whether it rejects a valid program

### Fixed by this slice

| what | measured symptom |
| --- | --- |
| `Some`, `Left`, `Right`, `Success`, `Failure` not `CASE` | `Some(1).copy(value = 2)` — "value copy is not a member of Some[Int]", and the same for the other four. `javap -p` shows `copy` and `copy$default$1` on each. |
| `::` was a second, empty class symbol beside `$colon$colon` | `val c: ::[Int]` — ":: does not take type parameters"; `new ::(1, Nil)` — "no matching overload for constructor ::". |
| A qualified constructor pattern resolved its class by looking the *last segment* up lexically | `case Ior.Left(a)` found `scala.util.Left`. Latent until `scala.util.Left` had `CASE`, which made the wrong class win the constructor arm over the extractor: 69 new errors in cats' `Ior.scala` / `IorT.scala` the moment the flag went on. |
| 1,396 of the 1,546 prelude methods that take parameters have no parameter symbols; another 150 (`prelude_seq::poly_in`) have them named `x$1` | every named argument on a library method the prelude declares: `List(1,2,3).mkString(sep = "-")` was "named arguments (method parameters not resolved)", `List(1,2).map(f = g)` was "unknown parameter name: f". A member the prelude does *not* declare — `List(1,2).padTo(len = 5, elem = 0)` — compiled, because it comes from the pickle. |

### Wrong, and it makes the compiler accept too much

Not fixed here: each would *add* diagnostics, and this slice deliberately
changed nothing that could turn a passing corpus test red for a reason unrelated
to its own subject.

* **`SEALED` missing on 15 classes** — `Option`, `List`, `Either`, `Try`,
  `Vector`, `Range`, `NumericRange`, `<:<`, `=:=`, `immutable.BitSet`,
  `immutable.Queue`, `mutable.ArraySeq`, `mutable.PriorityQueue`,
  `mutable.TreeMap`, `mutable.TreeSet`. Two consequences, both measured:
  `new Option[Int] {}` is accepted where scalac says "illegal inheritance from
  sealed class Option" (scala-rs has no such check at all), and
  `o match { case Some(x) => x }` draws no "match may not be exhaustive"
  warning. The warning also needs `Symbol::children` populated for the prelude
  hierarchies, which nothing does.
* **`ABSTRACT` missing on 10 classes** (`Option`, `List`, `Either`, `Try`,
  `Vector`, `Range`, `collection.Seq`, `immutable.BitSet`,
  `mutable.ArraySeq`, `collection.WithFilter`), and wrongly *set* on the five
  annotation classes `inline` / `noinline` / `volatile` / `transient` /
  `native` and on `switch` / `uncheckedVariance`.
* **`FINAL` set on 128 classes the library does not declare final**, because
  `prelude::class` sets it unconditionally. Mostly inert:
  `override_check::modifiers_are_known` excludes every symbol below
  `st.prelude_end` precisely so this cannot produce "cannot override final
  member". It is still read by `Check::is_final_like`, which decides whether a
  stable-identifier pattern's type and the scrutinee can be inhabited together
  (`Check::stable_pattern_compatible`) — a wrong `final` there rejects a
  pattern scalac accepts. No case of that was reproduced.
* **`collection.Seq` is not marked `TRAIT`**. Probed and found harmless:
  `class MySeq extends AbstractSeq[Int] with Seq[Int] with Marker` compiles and
  runs identically under both compilers.
* **`::` and `runtime.LazyRef` declare no type parameter** — `::` is fixed
  above; `LazyRef[T]` still has none.
* **Six members drop a `DEFAULTPARAM`** the library declares:
  `ArrayOps.{indexOf, indexWhere, lastIndexOf}` and the constructors of
  `mutable.{ArrayDeque, Queue, Stack}`.
* **63 prelude classes have no pickle under the name their JVM name implies** —
  mostly nested ones the prelude spells with `$` (`scala/Predef$ArrowAssoc`,
  `scala/util/Either$LeftProjection`, `scala/Option$WithFilter`). These are not
  necessarily defects: `PickleSupply::complete_named` has its own
  nested-spelling retry that this survey did not.
* No member the survey could match was missing `IMPLICIT`, and none dropped a
  by-name parameter.

## Adjacent defects the survey turned up, not part of this slice

* **A named application does not keep its written evaluation order.**
  `h(z = c, x = a, y = b)` on a plain `def h(x: String, y: String, z: String)`
  evaluates `a`, `b`, `c` — scalac evaluates `c`, `a`, `b` (SLS 6.6.1).
  `Check::record_named_arg_order` and `crate::named_eval_order` exist for
  exactly this and do not fire. Pre-existing, and reproducible with no library
  method involved.
* **`ArrayOps` has no `mkString` in the library**, so `Array(1,2,3).mkString(sep
  = "|")` still reports "method parameters not resolved": scalac reaches
  `IterableOnceOps.mkString` through `genericWrapArray`, and the prelude
  declares `mkString` on `ArrayOps` itself. Reading the names off a class that
  does not declare the member would be guessing, so the diagnostic stands.

## The `[B >: A]` members (`agent/lowerbound`, `agent/preludelb`)

The same "the hand-written member wins" rule costs the collections their
*lower-bounded* type parameters. `IterableOnceOps.reduce[B >: A]` arrives
through the pickle for every other collection; `List`'s came from
`prelude_seq`, which wrote it `((A, A) => A): A`, and a monomorphic signature
reads to the typer like a missing alternative — `no matching overload for
(Dog)Boolean with arguments (Cat)`.

`agent/lowerbound` closed `sorted` / `min` / `max` / `sum` / `product`.
`agent/preludelb` re-ran its probe as a two-directional accept/reject
comparison against real scalac 2.13.16 over 71 calls on `List`, `Map`, `Set`
and `Option`, and closed the rest in `prelude_lowbound.rs`:

| member | was | is |
|---|---|---|
| `List.contains` | `(A): Boolean` | `[A1 >: A](A1): Boolean` |
| `List.indexOf` | `(A): Int` | `[B >: A](B): Int`, and the `(B, Int)` arity added |
| `List.reduce` | `((A, A) => A): A` | `[B >: A]((B, B) => B): B` |
| `List.reduceLeft` | `((A, A) => A): A` | `[B >: A]((B, A) => B): B` |
| `List.reduceRight` | `((A, A) => A): A` | `[B >: A]((A, B) => B): B` |
| `List.toArray` | `(implicit ClassTag[A]): Array[A]` | `[B >: A](implicit ClassTag[B]): Array[B]` |
| `Map.+` | `((K, V)): Map[K, V]` | `[V1 >: V]((K, V1)): Map[K, V1]` |
| `Map.updated` | `(Any, Any): Map[K, V]` | `[V1 >: V](Any, V1): Map[K, V1]` |

Two of those the enumeration in `tests/BASELINE.md` did not name. `toArray`
was a sixth member of the `sorted` family's exact shape, and it failed in the
way that family failed — `found: Array[B]`, with `B` never instantiated,
because two candidates were in scope and only the pickled one fit the
argument. `Map.updated` was the worse of the two, because it **accepted** the
widening call and silently answered at the un-widened type: `Map` is covariant
in `V`, so `val m: Map[K, Animal] = md.updated(k, cat)` conforms either way and
only a narrow ascription separates them (`tests/fixtures/preludelb_bad.scala`).

Erasure is unchanged throughout, and this is the part that had to be measured
rather than argued, because `reduce` and `reduceLeft` take a *function* of the
widened type. `B`'s upper bound is `Any` exactly as `A`'s is, so the
descriptors stay `(Lscala/Function2;)Ljava/lang/Object;`,
`(Ljava/lang/Object;)Z`, `(Ljava/lang/Object;)I` and
`(Lscala/reflect/ClassTag;)Ljava/lang/Object;`. The class files emitted for
`List(1,2,3).reduce(_ + _)`, `reduceLeft`, `reduceRight`, `contains`, `indexOf`
and `toArray` are **byte-identical** to the branch point's, and every box and
unbox falls where scalac puts it. `crates/cli/tests/preludelb.rs` asserts this
with `javap -c` per call site rather than per class, because the `$anonfun$`
bodies do differ — for the unrelated reason in `specialization.md`, that nsc
gives the lambda an `apply$mcIII$sp` and this compiler emits a plain
`Function2` whose body unboxes. That gap shows on untouched members such as
`foldLeft` too.

### What the probe found and this slice did not fix

* **`Map.++` is absent by design.** `md ++ mc` is `found: Iterable[Product]
  required: Map[String, Animal]`. `prelude_coll::add_immutable_map_extra`
  declines to declare it, with a recorded runtime reason (the inherited
  `IterableOps.++` builds through `immutable.Iterable`'s factory and threw
  `ClassCastException` in both the `MapN` and `HashMap` cases). That reason is
  worth re-testing, but it is not a lower-bound question.
* **`Map`'s key parameters are `Any`, not `K`.** `md.apply(1)`, `md.get(1)`,
  `md.updated(1, dog)`, `md.getOrElse(1, dog)` and `md.contains(1)` on a
  `Map[String, Dog]` are all accepted here and all rejected by nsc. This is the
  deliberate approximation `prelude_ovl3::widen_map_get_or_else` already
  records; tightening it touches five members at once and is independent of the
  bound.
* **A lower bound that names another variable of the same call is not solved.**
  `def put[V, V1 >: V](m: List[V], v: V1)` called as `put(dogs, cat)` gives
  `inferred type arguments [Dog,Cat] do not conform to method put's type
  parameter bounds [V,V1 >: V]`: `V1` is solved from its argument alone instead
  of being joined with the still-undetermined `V`. Writing the bound as a
  concrete class (`[V1 >: Dog]`) works, and nsc infers `V1 = Animal` for both.
  Pre-existing, reproduced identically on the branch point, and unrelated to
  the prelude — the method is defined in source.
