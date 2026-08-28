object Main {
  def main(args: Array[String]): Unit = {
    val s = Set(1, 2, 3)
    println(s.contains(2))
    println(s.contains(99))
    val s2 = s + 4
    println(s2.size)
    val s3 = s - 1
    println(s3.size)
    val s4 = s ++ Set(4, 5)
    println(s4.size)
    println(s.size)
    println(s.isEmpty)
    println(s.nonEmpty)
    val f = s.filter(x => x > 1)
    println(f.size)
    val m = s.map(x => x * 10)
    println(m.size)
    println(s.toList.length)
    println(s.toSeq.length)
    val it = s.iterator
    println(it.hasNext)
    println(s.mkString(","))
    println(s.mkString("[", ",", "]"))
    println(s.head >= 1)
  }
}
