import scala.collection.immutable.SortedSet

object Main {
  def main(args: Array[String]): Unit = {
    val a = SortedSet(1270901784)
    val b = SortedSet(-1)
    val out: SortedSet[Int] = a.lazyZip(b).map((x, y) => x + y)
    println(out.mkString(","))
  }
}
