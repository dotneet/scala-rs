trait W {
  implicit def algebra: String = "base"
}
class C extends W {
  def algebra(x: Int): Int = x + 1
  def read: String = implicitly[String]
}
object Main {
  def main(args: Array[String]): Unit = {
    val c = new C
    println(c.read)
    println(c.algebra(4))
  }
}

