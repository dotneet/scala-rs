object Main {
  def main(args: Array[String]): Unit = {
    val d = scala.collection.mutable.ArrayDeque.empty[Int]
    d += 1
    d += 2
    d.prepend(0)
    println(d(0))
    println(d(1))
    println(d(2))
    val e = scala.collection.mutable.ArrayDeque(3, 4)
    e += 5
    println(e(0))
    println(e(1))
    println(e(2))
  }
}
