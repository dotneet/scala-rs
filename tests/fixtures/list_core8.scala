object Main {
  def main(args: Array[String]): Unit = {
    val xs = List(3, 1, 3)
    val arr: Array[Int] = xs.toArray
    println(arr.length)
    println(arr(0) + arr(1) + arr(2))
    println(xs.toVector)
    println(xs.toSeq)
    println(xs.toList.mkString(","))
    println(xs.toSet.contains(3))
    println(xs.toSet.contains(9))
    val ws = List("b", "a")
    val wa: Array[String] = ws.toArray
    println(wa(0) + wa(1))
    println(ws.toVector)
    println(xs.iterator.next())
    println(xs.grouped(2).toList.mkString(";"))
  }
}
