object Main {
  def main(args: Array[String]): Unit = {
    val m = scala.collection.mutable.LinkedHashMap.empty[Int, String]
    m.update(1, "a")
    m(2) = "b"
    m += (3 -> "c")
    println(m.apply(1))
    println(m(2))
    m.foreach(kv => println(kv._1))
    val n = scala.collection.mutable.LinkedHashMap(4 -> "x", 5 -> "y")
    n.foreach(kv => println(kv._1))
  }
}
