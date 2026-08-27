object Main {
  def main(args: Array[String]): Unit = {
    val b = scala.collection.immutable.BitSet(3, 1, 2)
    println(b.contains(1))
    println(b.contains(4))
    b.foreach(x => println(x))
  }
}
