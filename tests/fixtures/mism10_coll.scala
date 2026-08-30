// `collect` on a sorted map, both ways of writing the partial function.
//
//  * `SortedMapOps.collect[K2, V2](pf: PartialFunction[(K, V), (K2, V2)])
//    (implicit Ordering[K2])` reaches the literal with its undetermined
//    variables opened to their bounds -- `PartialFunction[(Int, String),
//    (Any, Any)]`. A *bare* variable in that position was already left open so
//    the case bodies decide it; one inside a tuple was not, the bodies came
//    back `(Any, Any)`, and the call asked for `Ordering[Any]`.
//
//  * With the partial function written as a value, `TreeMap.collect` resolved
//    to `MapOps.collect(pf)` -- the member some earlier `Map.collect` in the
//    same file had installed on `Map` -- and the call went out as
//    `IterableOps.collect`, whose default builds through `iterableFactory`.
//    `TreeMap(…).collect(pf)` *returned a `List`*, with no diagnostic
//    anywhere, and which of the two you got depended on whether a plain
//    `Map.collect` appeared earlier in the file.

import scala.collection.immutable.{TreeMap, TreeSet}

object Main {
  def main(args: Array[String]): Unit = {
    val plain = Map(1 -> "a", 2 -> "bb")
    println(plain.collect { case (k, v) => (k * 10, v.length) })

    val m = TreeMap(2 -> "bb", 1 -> "a")
    println(m.collect { case (k, v) => (k * 10, v.length) })
    // The narrowed static type is the point: `collect` has to be the sorted
    // one, or this assignment is a ClassCastException.
    val narrowed: TreeMap[Int, Int] = m.collect { case (k, v) => (k * 10, v.length) }
    println(narrowed)

    val pf: PartialFunction[(Int, String), (Int, Int)] = { case (k, v) => (k * 100, v.length) }
    println(m.collect(pf))
    val fromValue: TreeMap[Int, Int] = m.collect(pf)
    println(fromValue)

    val s = TreeSet(3, 1, 2)
    println(s.collect { case x if x > 1 => x * 2 })

    println(m.map { case (k, v) => (k + 1, v.length) })
    println(m.flatMap { case (k, v) => List((k + 100, v)) })
  }
}
