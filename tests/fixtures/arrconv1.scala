object Main {
  def main(args: Array[String]): Unit = {
    val xs = Array(3, 1, 2, 1)
    println(xs.toList)
    println(xs.toSeq)
    println(xs.toIndexedSeq)
    println(xs.toSet)
    println(xs.toVector)
    println(xs.groupBy(x => x % 2).view.mapValues(_.size).toMap)
    println(xs.sorted.toList)
    println(xs.sortBy(x => -x).toList)
    println(xs.sortWith((a, b) => a > b).toList)
    println(xs.mkString)
    println(xs.mkString(","))
    println(xs.mkString("[", ",", "]"))
  }
}
