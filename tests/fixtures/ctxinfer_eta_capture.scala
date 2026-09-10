class Converter(prefix: String) {
  def convert(n: Int)(implicit suffix: String): String = prefix + n + suffix
}
object Main {
  var calls: Int = 0
  def receiver: Converter = { calls += 1;
 new Converter("v") }
  def main(args: Array[String]): Unit = {
    implicit val suffix: String = "!"
    val f: Int => String = receiver.convert
    println(calls)
    println(f(2))
    println(f(3))
    println(calls)
  }
}

