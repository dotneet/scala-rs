// A local implicit class closes over another local of the enclosing method.
object Main {
  def main(a: Array[String]): Unit = {
    val factor = 10
    implicit class F(val n: Int) { def scaled: Int = n * factor }
    println(3.scaled)
  }
}
