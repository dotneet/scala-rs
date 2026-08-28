object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(1, 2, 3)
    // `map` is truly polymorphic: the element type follows the lambda.
    val ys: List[Int] = xs.map(x => x * 2)
    println(ys.mkString(","))
    val ss: List[String] = xs.map(x => "n" + x)
    println(ss.mkString(","))
    println(ss.head.length)
    val ls: List[Long] = xs.map(x => x.toLong * 10L)
    println(ls.sum)
    val nested: List[List[Int]] = xs.map(x => List(x, x))
    println(nested.mkString(";"))
    println(xs.flatMap(x => List(x, x * 10)).mkString(","))
    val fs: List[String] = xs.flatMap(x => List("a" + x, "b" + x))
    println(fs.mkString(","))
    val pf: PartialFunction[Int, String] = { case 1 => "one"; case 3 => "three" }
    val cs: List[String] = xs.collect(pf)
    println(cs.mkString(","))
    println(xs.zip(List("a", "b", "c")).mkString(","))
    println(xs.zipWithIndex.mkString(","))
  }
}
