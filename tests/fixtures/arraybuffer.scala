object Main {
  def main(args: Array[String]): Unit = {
    val b = scala.collection.mutable.ArrayBuffer(1, 2)
    b += 3
    println(b(0))
    b(1) = 9
    println(b(1))
    println(b(2))
  }
}
