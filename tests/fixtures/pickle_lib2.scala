// Pickle-supplied members that need more than a plain signature:
//
//   * `sorted` / `max` / `min` are `[B >: A](implicit Ordering[B])`. Nothing at
//     the call site determines `B`, so it is pinned to its lower bound `A`,
//     which is what scalac infers; the implicit `Ordering[Int]` then resolves.
//   * `toList` on `Option` returns `scala.package.List`, a package-object type
//     alias, expanded through `scala/package.class`'s own pickle.
//   * `scanLeft` / `patch` / `padTo` are curried or multi-argument.
object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(3, 1, 2)
    println(xs.sorted)
    println(xs.sortBy(x => x))
    println(xs.sortWith((a, b) => a < b))
    println(xs.max)
    println(xs.min)
    println(xs.maxBy(x => x))
    println(xs.toVector)
    println(xs.toSet.toList.sorted)
    println(xs.toArray.length)
    println(xs.scanLeft(0)((a, b) => a + b))
    println(xs.zip(xs))
    println(xs.padTo(5, 0))
    println(xs.updated(0, 9))
    println(xs.patch(0, List(7), 1))
    println(xs.indexWhere(_ > 1))
    println(xs.tails.toList)
    println(xs.combinations(2).toList)
    println(xs.permutations.toList.length)

    val o: Option[Int] = Some(3)
    println(o.exists(_ > 2))
    println(o.forall(_ > 2))
    println(o.contains(3))
    println(o.filter(_ > 2))
    println(o.toList)

    val v = Vector(1, 2, 3)
    println(v.filter(_ > 1))
    println(v.mkString("-"))

    println(Map(1 -> "a", 2 -> "b").keySet)
    println(xs.zipWithIndex)
    println(xs.grouped(2).toList)
    println(xs.sliding(2).toList)
  }
}
