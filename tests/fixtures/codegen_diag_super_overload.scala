trait OverloadBase {
  def foo(i: Int): String = "int"
  def foo(s: String): String = "string"
}

trait OverloadLayer extends OverloadBase {
  def helperInt: String = super.foo(1)
  def helperString: String = super.foo("x")
}

class OverloadBoth extends OverloadLayer

object OverloadMain {
  def main(args: Array[String]): Unit = {
    val b = new OverloadBoth()
    println(b.helperInt)
    println(b.helperString)
  }
}
