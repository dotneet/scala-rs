object Main {
  def main(args: Array[String]): Unit = {
    val s = scala.collection.mutable.HashSet.empty[Int]
    s += 1
    s += 2
    println(s.contains(1))
    println(s.contains(0))
    val t = scala.collection.mutable.HashSet(3, 4)
    println(t.contains(3))
    println(t.contains(9))
  }
}
