// Compiled separately (by scalac and by scala-rs) and used from
// `erascg_binlib_use.scala`: the descriptors a primitive bound and an
// inherited alias bound erase to must agree across the two compilers.
package erascglib

class P[A <: Int](val a: A) {
  def get: A = a
  def twice(x: A): Int = x + x
  var v: A = a
}
trait X { type T <: Int; def f(t: T): T }
trait TI[A <: Long] { def h(a: A): A }
object Fns {
  def id[A <: Int](a: A): A = a
  def mk[A <: Char](a: A): List[A] = List(a, a)
  def arr[A <: Int](a: Array[A]): A = a(0)
}
trait Base[A] { type B = A }
class C extends Base[String] {
  class D { def foo[B1 <: B](b: B1): Int = b.length }
  // Built here rather than by the client's `new c.D`: what is being checked
  // is `foo`'s descriptor across compilers, not inner-class instantiation.
  def mkD: D = new D
}
class SB[A <: String](val a: A) { def len: Int = a.length; def get: A = a }

// Names already used by the standard library, plus two same-name siblings.
object First {
  case class Regex()
  object Regex { def label: String = "first" }
}
object Second {
  case class Regex(value: Int)
  object Regex { def label: String = "second" }
}
object Views {
  def size[T](value: T)(implicit ev: T => Iterable[_]): Int = ev(value).size
  def integers(ev: String => Iterable[_ <: Int]): Int = ev("x").size
  def nested(xs: Box[List[_]]): Int = xs.value.size
  def mixed(xs: Box[(_, List[_])]): Int = xs.value._2.size
}
class Box[A](val value: A)
final class ValueBox[A](val value: A) extends AnyVal

object Symbolic {
  final case class ::[A, B](head: A, tail: B)
}
object SymbolicUse {
  def identity[A, B](xs: Symbolic.::[A, B]): Symbolic.::[A, B] = xs
}
