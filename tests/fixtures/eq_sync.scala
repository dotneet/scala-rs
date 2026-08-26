class Box
object Main {
  def main(args: Array[String]): Unit = {
    val a = new Box()
    val b = new Box()
    println(a.eq(a))
    println(a.eq(b))
    println(a.ne(b))
    val n: Int = a.synchronized { 41 }
    println(n + 1)
  }
}
