class C {
  protected[C] val n: Int = 1
}
object Main {
  def peek(c: C): Int = c.n
  def main(args: Array[String]): Unit = {
    println(peek(new C()))
  }
}
