object Main {
  def main(args: Array[String]): Unit = {
    val m = Map("a" -> 1, "b" -> 2, "c" -> 3)
    println(m("a"))
    println(m.get("z"))
    println(m.getOrElse("z", -1))
    println(m.contains("a"))
    println(m.contains("zzz"))
    println(m.keys.toList.length)
    println(m.values.toList.length)
    println(m.keySet.size)
    val m2 = m + ("d" -> 4)
    println(m2.size)
    val m3 = m - "a"
    println(m3.size)
    println(m.size)
    println(m.isEmpty)
    println(m.nonEmpty)
    val f = m.filter(p => p._2 > 1)
    println(f.size)
    println(m.toList.length)
    println(m.toSeq.length)
    println(m.mkString(","))
    println(m.head._1.length > 0)
    var sum = 0
    m.foreach(p => sum += p._2)
    println(sum)
    println(m.foldLeft(0)((acc, p) => acc + p._2))
    val withDefault = m.withDefaultValue(0)
    println(withDefault("nope"))
    val updated = m.updated("g", 7)
    println(updated.size)
  }
}
