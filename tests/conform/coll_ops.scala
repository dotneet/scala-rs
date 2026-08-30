object Main {
  def main(a: Array[String]): Unit = {
    val xs = List(1, 2, 3, 4, 5)
    println(xs.grouped(2).toList)
    println(xs.sliding(2).toList)
    println(xs.scanLeft(0)(_ + _))
    println(xs.foldRight(List.empty[Int])((x, acc) => x * 2 :: acc))
    println(xs.partition(_ % 2 == 0))
    println(xs.span(_ < 3))
    println(xs.zipWithIndex.map { case (x, i) => s"$i:$x" }.mkString(","))
    println(xs.groupBy(_ % 2).toList.sortBy(_._1))
  }
}
