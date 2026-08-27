object Main {
  def main(args: Array[String]): Unit = {
    val s = scala.collection.mutable.LinkedHashSet.empty[Int]
    s += 1
    s += 2
    s += 1
    println(s.contains(1))
    println(s.contains(0))
    s.foreach(x => println(x))
    val t = scala.collection.mutable.LinkedHashSet(3, 4)
    t.foreach(x => println(x))
  }
}
