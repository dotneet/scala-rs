// Members reached through linearization rather than a breadth-first walk, and
// members whose signatures mention library classes the prelude never declared.
//
// `Map#map` and `Set#map` are declared on `IterableOps` returning its opaque
// `C`. Which binding of `C` you get depends on the order parents are searched:
// SLS 5.1.2 says a later parent wins, so `C` comes through `MapOps` / `SetOps`
// (`Map[K2, V2]` / `Set[A]`) and not through `Iterable` (`Iterable[...]`).
// A breadth-first walk gets that backwards and lands on a type the symbol
// table does not have, so the member used to be declined outright.
object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(1, 2, 3, 4)
    println(Map("a" -> 1).map { case (k, v) => (k, v + 1) })
    println(Map("a" -> 1, "b" -> 2).filter(_._2 > 1))
    println(Set(1, 2, 3).filter(_ > 1))
    println(Set(1, 2, 3).map(_ * 2))
    println((1 to 10).filter(_ % 2 == 0).map(_ * 2))
    println(Vector(1, 2, 3).map(_ + 1))

    println(xs.sliding(2, 2).toList)
    println(xs.view.map(_ * 2).toList)
    println(xs.iterator.toList)
    println(xs.toSeq.length)
    println(xs.flatMap(x => List(x, x)))
    println(xs.foldRight(0)(_ + _))
    println(xs.reduce(_ + _))
    println(xs.reduceLeft(_ + _))
    println(xs.copyToArray(new Array[Int](4)))
    println(xs.mkString)
  }
}
