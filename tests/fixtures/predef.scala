object Main {
  def main(args: Array[String]): Unit = {
    assert(true)
    require(1 > 0)
    println("42".length)
    println("42".toInt)
    val t = 1 -> "a"
    println(t._1)
    println(t._2)
    try {
      ???
    } catch {
      case _: RuntimeException => println("nyi")
    }
  }
}
