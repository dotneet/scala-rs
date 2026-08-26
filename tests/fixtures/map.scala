object Main {
  def main(args: Array[String]): Unit = {
    val m0 = Map.empty
    val m = m0.updated(1, "a")
    println(m.apply(1))
    println(m.get(1).get)
    m.foreach(kv => println(kv._2))
    val m2 = m.updated(2, "b")
    println(m2.apply(2))
  }
}
