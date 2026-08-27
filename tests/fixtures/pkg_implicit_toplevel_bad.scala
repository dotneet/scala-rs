implicit class Rich(n: Int) {
  def twice: Int = n * 2
}
object Main {
  def main(args: Array[String]): Unit = {
    println(2.twice)
  }
}
