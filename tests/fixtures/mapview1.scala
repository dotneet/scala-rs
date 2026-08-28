object Main {
  def main(args: Array[String]): Unit = {
    val m = Map("a" -> 1, "b" -> 2, "c" -> 3)
    val v = m.view
    println(v.mapValues(_ * 10).toMap)
    println(v.filterKeys(_ != "b").toMap)
    println(v.keys.toList)
    println(v.values.toList)
    println(v.size)
    println(v.isEmpty)
    println(v.toList)
    println(v.toSeq)
    var total = 0
    v.foreach(kv => total += kv._2)
    println(total)

    val grouped = Array(1, 1, 2, 2, 2, 3).groupBy(x => x)
    println(grouped.view.mapValues(_.size).toMap)
  }
}
