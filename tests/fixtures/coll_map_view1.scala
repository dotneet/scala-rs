object Main {
  def main(args: Array[String]): Unit = {
    val m = Map("a" -> 1, "b" -> 2, "c" -> 3)
    val doubled = m.view.mapValues[Int](v => v * 2)
    println(doubled.toList.length)
    println(doubled.mkString(","))
  }
}
