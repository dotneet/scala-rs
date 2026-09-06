trait LinearBase {
  def foo(i: Int): String = "base-int"
  def foo(s: String): String = "base-string"
}

trait LinearMiddle extends LinearBase {
  override def foo(s: String): String = "middle-string"
}

trait LinearLayer extends LinearBase {
  def helper: String = super.foo(1)
}

class LinearClient extends LinearBase with LinearMiddle with LinearLayer

object LinearMain {
  def main(args: Array[String]): Unit = println(new LinearClient().helper)
}
