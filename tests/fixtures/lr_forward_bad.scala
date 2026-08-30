// Only a `lazy val` may be forward-referenced inside a block. An eager one
// still has to be written before it is read.
object Main {
  def main(args: Array[String]): Unit = {
    val a: Int = b + 1
    val b: Int = 2
    println(a + b)
  }
}
