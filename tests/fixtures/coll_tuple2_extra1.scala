object Main {
  def main(args: Array[String]): Unit = {
    val t = "a" -> 1
    println(t._1)
    println(t._2)
    val s = t.swap
    println(s._1)
    println(s._2)
    println(t.toString)
    println(t)
  }
}
