import scala.language.implicitConversions
object Main {
  class Base[+A](val text: String) {
    def bar[B >: A](other: => Base[B], suffix: String = ""): Base[B] =
      new Base[B](text + other.text + suffix)
  }
  implicit def fromString(s: String): Base[String] = new Base[String](s)
  def rep[A](p: => Base[A]): Base[A] = p
  def main(args: Array[String]): Unit = {
    val result: Base[Any] = rep("foo" bar "bar") bar "sth"
    println(result.text)
    implicit def numberToString(i: Int): String = "converted"
    val m = Map("x" -> "ok")
    println(m.getOrElse("missing", 3))
    println(m.getOrElse("missing", { 4 }))
  }
}
