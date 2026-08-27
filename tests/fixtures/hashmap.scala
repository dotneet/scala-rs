object Main {
  def main(args: Array[String]): Unit = {
    val m = scala.collection.mutable.HashMap.empty[Int, String]
    m.update(1, "a")
    m(2) = "b"
    m += (3 -> "c")
    println(m.apply(1))
    println(m(2))
    println(m.get(3).get)
    val n = scala.collection.mutable.HashMap(1 -> "x", 2 -> "y")
    println(n(1))
    println(n.get(2).get)
  }
}
