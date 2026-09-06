import scala.language.implicitConversions
trait Foo[A] {
  implicit def convert(a: A): Ordered[A]
  class Filter(f: A => Boolean) extends Foo[A] {
    implicit def convert(a: A): Ordered[A] = Foo.this.convert(a)
    def accepts(a: A): Boolean = f(a)
  }
  class Range(x: A, y: A) extends Filter(a => a.compare(x) >= 0 && a.compare(y) < 0)
  def exercise(lo: A, hi: A, before: A, inside: A): Unit = {
    val r = new Range(lo, hi)
    println(r.accepts(before))
    println(r.accepts(lo))
    println(r.accepts(inside))
    println(r.accepts(hi))
  }
}
object Numbers extends Foo[Int] {
  implicit def convert(a: Int): Ordered[Int] = new Ordered[Int] {
    def compare(b: Int): Int = a - b
  }
}
object Main {
  def main(args: Array[String]): Unit = {
    Numbers.exercise(2, 5, 1, 4)
  }
}
