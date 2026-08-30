// Two equally specific local implicit conversions must be reported ambiguous,
// same as scalac -- not "silently pick one" and not "not found".
object Main {
  def main(a: Array[String]): Unit = {
    implicit def i2sA(n: Int): String = "a" + n
    implicit def i2sB(n: Int): String = "b" + n
    val str: String = 5
    println(str)
  }
}
