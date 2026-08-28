object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(3, 1, 4)
    println(xs.mkString)
    println(xs.mkString("-"))
    println(xs.mkString("[", ", ", "]"))
    println(xs.sum)
    println(xs.product)
    println(xs.min)
    println(xs.max)
    println(xs.minBy(x => 0 - x))
    println(xs.maxBy(x => 0 - x))
    val ls = List(3L, 1L, 4L)
    println(ls.sum)
    println(ls.max)
    val ds = List(1.5, 2.5)
    println(ds.sum)
    val ss = List("pear", "fig", "apple")
    println(ss.mkString("/"))
    println(ss.min)
    println(ss.max)
    println(ss.maxBy(s => s.length))
  }
}
