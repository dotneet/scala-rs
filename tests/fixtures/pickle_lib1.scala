// Members supplied from scala-library's own ScalaSignature pickles rather than
// from the hand-written prelude. `filter` is declared on IterableOps, `mkString`
// / `exists` / `forall` / `count` / `find` on IterableOnceOps, `take` / `drop` /
// `startsWith` on SeqOps; all are reached through List's parents.
object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(1, 2, 3, 4)
    println(xs.filter(_ > 1))
    println(xs.filterNot(_ > 1))
    println(xs.count(_ > 1))
    println(xs.exists(_ > 3))
    println(xs.forall(_ > 0))
    println(xs.take(2))
    println(xs.drop(2))
    println(xs.takeWhile(_ < 3))
    println(xs.dropWhile(_ < 3))
    println(xs.reverse)
    println(xs.mkString(","))
    println(xs.mkString("[", ";", "]"))
    println(xs.contains(3))
    println(xs.indexOf(3))
    println(xs.init)
    println(xs.last)
    println(xs.distinct)
    println(xs.startsWith(List(1, 2)))
    println(xs.splitAt(2))
    println(xs.partition(_ > 2))
    println(xs.span(_ < 3))
    println(xs.slice(1, 3))
    println(xs.headOption)
    println(xs.lastOption)
    println(xs.find(_ > 2))
  }
}
