// Handle Array generically (a hand-rolled utility with a ClassTag).
import scala.reflect.ClassTag

object Buf {
  def repeat[T: ClassTag](x: T, n: Int): Array[T] = {
    val a = new Array[T](n)
    var i = 0
    while (i < n) { a(i) = x; i += 1 }
    a
  }

  def concat[T: ClassTag](a: Array[T], b: Array[T]): Array[T] = {
    val out = new Array[T](a.length + b.length)
    Array.copy(a, 0, out, 0, a.length)
    Array.copy(b, 0, out, a.length, b.length)
    out
  }

  def show[T](a: Array[T]): String = a.mkString("[", ",", "]")
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Buf.show(Buf.repeat(3, 4)))
    println(Buf.show(Buf.repeat("z", 2)))
    println(Buf.show(Buf.concat(Array(1, 2), Array(3))))
    println(Buf.show(Buf.concat(Array("a"), Array("b", "c"))))
    println(Buf.show(Buf.repeat((1, "one"), 2)))
    println(Buf.concat(Array(1, 2), Array(3)).sum)
    println(Buf.show(Array(1, 2) ++ Array(3, 4)))
    println(Buf.show(Array("a", "b").reverse))
  }
}
