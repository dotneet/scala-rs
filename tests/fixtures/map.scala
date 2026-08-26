object Main {
  def main(args: Array[String]): Unit = {
    val m = Map(1 -> "a", 2 -> "b")
    println(m.apply(1))
    println(m.apply(2))
    println(m.get(1).get)
    m.foreach(kv => println(kv._2))
  }
}
