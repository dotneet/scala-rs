object Main {
  implicit class Rich(n: Int) {
    def twice: Int = n * 2
  }
  def main(args: Array[String]): Unit = {
    val n: Int = 2.twice
  }
}
