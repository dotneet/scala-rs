object Main {
  def main(args: Array[String]): Unit = {
    val m = Map("a" -> 1)
    val pair = m ++ List("b" -> 2)
    val scalar = m ++ List(7, 8)
    println(pair("b"))
    println(scalar.toList.mkString("|"))
    val mapped = m.map { case (_, v) => v + 1 }
    println(mapped.head)
    val collected = m.collect { case (_, v) => v + 2 }
    println(collected.head)
  }
}
