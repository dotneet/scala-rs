trait GenericBase[T] {
  def foo(t: T): String = "base"
}

trait GenericMiddle extends GenericBase[Int] {
  def foo(s: String): String = "middle-string"
}

trait GenericLayer extends GenericBase[Int] {
  def helper: String = super.foo(1)
}

class GenericClient extends GenericBase[Int] with GenericMiddle with GenericLayer

object GenericMain {
  def main(args: Array[String]): Unit = println(new GenericClient().helper)
}
