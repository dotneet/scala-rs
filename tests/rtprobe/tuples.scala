// Tuples: toString, equality with mixed numeric types, swap, productIterator,
// tuple patterns, specialized Tuple2 fields, zip/unzip, and tuple of units.
object Main {
  def swapGen[A, B](t: (A, B)): (B, A) = t.swap
  def firstOf[A](t: (A, A)): A = t._1
  def main(args: Array[String]): Unit = {
    val t = (1, "two", 3.0, '4', 5L, true)
    println(t); println(t._1 + t._3 + t._5); println(t.productArity); println(t.productIterator.mkString("|"))
    val p = (1, 2)
    println(p.swap); println(swapGen(p)); println(swapGen(("a", 1.5))); println(firstOf((7, 8)))
    println(p == ((1, 2))); println((1, 2) == ((1L, 2L))); println((1, 2).hashCode == ((1, 2)).hashCode)
    val (a, b) = p
    println(a + b)
    val ((x, y), z) = ((1, 2), 3)
    println(x * 100 + y * 10 + z)
    val tl = List((1, "a"), (2, "b"), (3, "c"))
    println(tl.unzip); println(tl.map(_._1).sum); println(tl.toMap); println(tl.map { case (n, s) => s * n })
    println((1 to 3).zip("abc")); println(List(1, 2).zip(List("x")))
    val u = ((), ())
    println(u)
    val d = (1.5, 2.5)
    println(d._1 + d._2)
    val cp = (1, 2).copy(_2 = "two")
    println(cp)
    println(Tuple2(1, 2).getClass.getSimpleName)
    println((1, 2, 3).productElement(2))
    val nested = ((1, (2, (3, 4))), 5)
    println(nested._1._2._2._1)
    def sumPair(pr: (Int, Int)): Int = pr._1 + pr._2
    println(sumPair((3, 4)))
    println(List((2, "b"), (1, "z"), (2, "a"), (1, "a")).sorted)
    val m = scala.collection.mutable.Map.empty[(Int, Int), String]
    m((1, 2)) = "one-two"; m((1, 2)) = "again"; m((2, 1)) = "two-one"
    println(m.toList.sorted)
    val fn: ((Int, Int)) => Int = { case (q, r) => q * r }
    println(fn((6, 7)))
    val f2 = (q: Int, r: Int) => q - r
    println(f2.tupled((10, 3)))
  }
}
