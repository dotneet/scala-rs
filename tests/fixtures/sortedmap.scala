object Main {
  def main(args: Array[String]): Unit = {
    val m = scala.collection.immutable.SortedMap(3 -> "c", 1 -> "a", 2 -> "b")
    m.foreach(kv => println(kv._1))
    println(m.apply(1))
    println(m.get(2).get)
    val t = scala.collection.immutable.TreeMap(5 -> "e", 4 -> "d")
    t.foreach(kv => println(kv._1))
    println(t(4))
  }
}
