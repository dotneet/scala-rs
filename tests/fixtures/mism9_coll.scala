// A sorted collection's `map` / `flatMap` / `collect`, and `foreach` with a
// function that returns something other than `Unit`.
//
// 2.13 declares `map[B](f)(implicit ord: Ordering[B]): CC[B]` on
// `SortedSetOps` and `map[B](f): CC[B]` on `IterableOps`. Both were pulled
// down onto `TreeSet` by the pickle reader, which throws the owners away, so
// neither was more specific and every `TreeSet.map(f)` was `ambiguous
// overload`.
//
// `IterableOnceOps.foreach[U](f: A => U): Unit` is polymorphic in the
// function's result; the prelude wrote `A => Unit`, which a function *value*
// (as opposed to a literal, whose body is discarded) does not conform to.

import scala.collection.immutable.{TreeMap, TreeSet}

object Main {
  def each[R](xs: Range)(f: Int => R): Unit = xs.foreach(f)

  def main(args: Array[String]): Unit = {
    val ts: TreeSet[Int] = TreeSet(3, 1, 2)
    println(ts.map(_ + 1))
    println(ts.flatMap(x => List(x, x + 10)))
    println(ts.collect { case x if x > 1 => x * 2 })
    // The narrowed static type is the point: `Set` here would be a
    // `ClassCastException` at the assignment.
    val narrowed: TreeSet[Int] = ts.map(_ * 3)
    println(narrowed)

    val tm: TreeMap[String, Int] = TreeMap("b" -> 2, "a" -> 1)
    println(tm.map { case (k, v) => (k + "!", v + 1) })
    println(tm.flatMap { case (k, v) => List((k + "?", v)) })

    val seen = new StringBuilder
    each(1 to 3)(i => { seen.append(i); i * 2 })
    println(seen.toString)
  }
}
