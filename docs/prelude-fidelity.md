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
