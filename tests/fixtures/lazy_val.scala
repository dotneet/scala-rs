object Main {
  var n: Int = 0
  lazy val x: Int = {
    n = n + 1
    41 + 1
  }
  def main(args: Array[String]): Unit = {
    println(n)
    println(x)
    println(x)
    println(n)
  }
}
