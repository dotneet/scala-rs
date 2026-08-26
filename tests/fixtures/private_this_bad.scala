class C {
  private[this] val n: Int = 1
  def steal(other: C): Int = other.n
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().steal(new C()))
  }
}
