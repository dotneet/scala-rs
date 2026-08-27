object Main {
  def main(args: Array[String]): Unit = {
    val s = scala.collection.immutable.SortedSet(3, 1, 2)
    s.foreach(x => println(x))
    println(s.contains(1))
    println(s.contains(9))
    val t = scala.collection.immutable.TreeSet(5, 4, 6)
    t.foreach(x => println(x))
    println(t.contains(4))
  }
}
