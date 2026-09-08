// `SortedMap` and `SortedSet` keep their ordering through `map`, `flatMap`
// and `collect`.
//
// `SortedMapOps` overloads all three with an `(implicit Ordering[K2])` clause
// returning the *sorted* collection, next to the `MapOps` ones it inherits.
// They have the same explicit parameters, so only one copy survives the
// pickle's completion, and nothing but the owner separates them -- nsc's
// `isAsSpecific` looks through an implicit clause. The one kept was whichever
// the walk offered first, which is `MapOps`, so `aSortedMap.map(f)` came back
// an unordered `Map`.
//
// So the ordering is *reversed* here and every line prints iteration order:
// a result built through the unordered overload cannot print it, and the
// ascriptions pin the static type as well.
import scala.collection.immutable.{SortedMap, SortedSet}

object Main {
  implicit val down: Ordering[Int] = Ordering.Int.reverse

  def main(args: Array[String]): Unit = {
    val m: SortedMap[Int, String] = SortedMap(1 -> "a", 3 -> "c", 2 -> "b")
    println(m.mkString(" "))

    val mapped: SortedMap[Int, String] = m.map { case (k, v) => (k * 10, v.toUpperCase) }
    println(mapped.mkString(" "))

    val flat: SortedMap[Int, String] = m.flatMap { case (k, v) => List((k, v), (k + 100, v)) }
    println(flat.keys.mkString(","))

    val picked: SortedMap[Int, String] = m.collect { case (k, v) if k != 2 => (k, v) }
    println(picked.keys.mkString(","))

    // No ascription: here the *value* is the whole evidence, so this one has
    // to be big enough that the unordered answer is visibly unordered. Six
    // entries makes an `immutable.HashMap`, which prints `10,6,9,7,11,8`;
    // under five, `Map.map` builds a `MapN` that keeps insertion order and
    // the wrong overload prints the right answer by accident.
    val big: SortedMap[Int, String] =
      SortedMap(1 -> "a", 2 -> "b", 3 -> "c", 4 -> "d", 5 -> "e", 6 -> "f")
    val loose = big.map { case (k, v) => (k + 5, v) }
    println(loose.keys.mkString(","))

    // `SortedSet`'s own `map` / `collect` are *not* fixed here and are
    // deliberately not exercised: `immutable.SortedSet` is hand-written in the
    // prelude with no `map` of its own, so the selection lands on the
    // prelude's `Set.map` and `Check::rebuild_from_receiver` narrows the
    // result to `SortedSet[B]` without the `Ordering[B]` witness that
    // `SortedSetOps.map` needs. The call goes out as
    // `IterableOps.map(Function1)` and the value is a `Set$Set3`, so the
    // narrowing is a `ClassCastException` at the first use -- on this binary
    // and on the one before it alike. See `docs/cats.md`.
    val s: SortedSet[Int] = SortedSet(4, 1, 7)
    println(s.mkString(","))
    println(s.contains(4).toString + " " + s.contains(5).toString)
  }
}
