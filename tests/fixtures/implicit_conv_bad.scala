object Main {
  class Rich(n: Int) { def twice: Int = n * 2 }
  implicit def toRich(n: Int): Rich = new Rich(n)
  def main(args: Array[String]): Unit = {
    val n: Int = 2.twice
  }
}
