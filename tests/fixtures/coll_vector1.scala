object Main {
  def main(args: Array[String]): Unit = {
    val v = Vector(1, 2, 3, 4, 5)
    println(v(0))
    println(v.length)
    println(v.size)
    println(v.isEmpty)
    println(v.nonEmpty)
    println(v.head)
    val v2 = v.updated(0, 100)
    println(v2(0))
    val v3 = v :+ 6
    println(v3.length)
    v.foreach(x => print(x + " "))
    println()
    val m = v.map(x => x + 1)
    println(m.mkString(","))
    val f = v.filter(x => x > 2)
    println(f.mkString(","))
    println(v.toList)
    println(v.toSeq.length)
    val it = v.iterator
    println(it.hasNext)
    println(v.mkString(","))
    println(v.mkString("[", ",", "]"))
    println(v.foldLeft(0)((acc, x) => acc + x))
  }
}
