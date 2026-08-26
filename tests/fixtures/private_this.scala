class C {
  private[this] val n: Int = 41
  def get: Int = n
  def add: Int = this.n + 1
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().get)
    println(new C().add)
  }
}
