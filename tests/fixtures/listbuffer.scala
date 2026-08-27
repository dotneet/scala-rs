object Main {
  def main(args: Array[String]): Unit = {
    val b = scala.collection.mutable.ListBuffer(1, 2)
    b += 3
    println(b(0))
    println(b(1))
    println(b(2))
  }
}
