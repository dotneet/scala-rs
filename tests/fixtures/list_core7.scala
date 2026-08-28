object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(2, 3)
    println((1 :: xs).mkString(","))
    println((List(0, 1) ::: xs).mkString(","))
    println((1 +: xs).mkString(","))
    println((xs :+ 4).mkString(","))
    println((xs ++ List(4, 5)).mkString(","))
    println((xs ++: List(4, 5)).mkString(","))
    println((xs :++ List(4, 5)).mkString(","))
    println(xs.concat(List(9)).mkString(","))
    println(xs.updated(0, 7).mkString(","))
    val ys = List(1, 2, 3, 4)
    println(ys.splitAt(2))
    println(ys.span(x => x < 3))
    println(ys.partition(x => x % 2 == 0))
    println(ys.startsWith(List(1, 2)))
    println(ys.startsWith(List(2)))
    println(ys.endsWith(List(3, 4)))
    println(ys.endsWith(List(3)))
    // `::` is polymorphic in `B >: A`; the element type follows the argument.
    val zs: List[String] = "a" :: List("b")
    println(zs.mkString(","))
  }
}
