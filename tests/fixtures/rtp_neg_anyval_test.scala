// scalac: type AnyVal cannot be used in a type pattern or isInstanceOf test
// (twice: the pattern and the explicit test).
object Main {
  def main(args: Array[String]): Unit = {
    val one: Any = 1
    println(one.isInstanceOf[AnyVal])
    println(one match { case _: AnyVal => "anyval"; case _ => "ref" })
  }
}
