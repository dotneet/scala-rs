object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(3, 1, 4, 1, 5)
    println(xs.sorted.mkString(","))
    println(xs.sortBy(x => 0 - x).mkString(","))
    println(xs.sortWith((a, b) => a > b).mkString(","))
    println(xs.distinctBy(x => x % 2).mkString(","))
    println(xs.groupBy(x => x % 2))
    println(xs.grouped(2).toList)
    println(xs.sliding(2).toList)
    println(xs.sliding(3, 2).toList)
    val ws = List("pear", "fig", "apple")
    println(ws.sorted.mkString(","))
    println(ws.sortBy(w => w.length).mkString(","))
    println(ws.groupBy(w => w.length))
  }
}
