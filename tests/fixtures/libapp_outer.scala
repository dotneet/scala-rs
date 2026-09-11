// Members reached through an enclosing class, and views imported from a
// value: reductions of `scala.math.Integral`/`Numeric`, `SetOps`'
// `SubsetsItr` and `NumericRange`. The output is compared with scalac.
import scala.language.implicitConversions

// A bare inherited member written inside a nested class is the *outer*
// instance's (`SetOps.this.newSpecificBuilder` inside `SubsetsItr`).
trait Box[A] {
  def get: A
  def wrap(a: A): List[A] = List(a)
}

trait Holder[B] extends Box[B] {
  def items: List[B]
  class Inner {
    def first: B = get
    def both: List[B] = wrap(items.head) ++ wrap(get)
  }
  def viaMethod: List[B] = {
    val r = new Runnable { def run(): Unit = (); def g: B = get }
    List(r.g)
  }
}

class StrHolder extends Holder[String] {
  def get = "g"
  def items = List("i")
}

// An inner class of a generic trait is constructed through its prefix:
// `class IntegralOps(lhs: T) extends NumericOps(lhs)` inside `Integral[T]`,
// and `new num.NumericOps(x)` in `Numeric.infixNumericOps`.
trait MyNumeric[T] {
  def plus(x: T, y: T): T
  def minus(x: T, y: T): T
  def show(x: T): String = "<" + x + ">"
  class NumericOps(lhs: T) {
    def +(rhs: T) = plus(lhs, rhs)
    def -(rhs: T) = minus(lhs, rhs)
    def s: String = show(lhs)
  }
  implicit def mkNumericOps(lhs: T): NumericOps = new NumericOps(lhs)
}

trait MyIntegral[T] extends MyNumeric[T] {
  def quot(x: T, y: T): T
  class IntegralOps(lhs: T) extends NumericOps(lhs) {
    def /(rhs: T) = quot(lhs, rhs)
  }
  override implicit def mkNumericOps(lhs: T): IntegralOps = new IntegralOps(lhs)
  def plain(lhs: T): NumericOps = new NumericOps(lhs)
}

object IntIsMyIntegral extends MyIntegral[Int] {
  def plus(x: Int, y: Int) = x + y
  def minus(x: Int, y: Int) = x - y
  def quot(x: Int, y: Int) = x / y
}

object StrIsMyNumeric extends MyNumeric[String] {
  def plus(x: String, y: String) = x + y
  def minus(x: String, y: String) = x.stripSuffix(y)
}

// `import num._` offers `mkNumericOps(lhs: T)`, which is more specific than
// `Predef.any2stringadd[A](self: A)` -- even though `+` is only *inherited*
// by `IntegralOps` -- and the import still means the class's `num` inside a
// method whose own parameter is called `num` (`NumericRange.sum`).
class Range0[T](val start: T, val end: T, val step: T)(implicit num: MyIntegral[T]) {
  import num._
  def next: T = start + step
  def prev: T = end - step
  def half: T = (start + end) / step
  def sum[B >: T](n: Int)(implicit num: MyNumeric[B]): B = {
    var acc: B = start
    var i = start
    var idx = 1
    while (idx < n) {
      i = i + step
      acc = num.plus(acc, i)
      idx = idx + 1
    }
    acc
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    val h = new StrHolder
    val in = new h.Inner
    println(in.first)
    println(in.both)
    println(h.viaMethod)
    println(IntIsMyIntegral.mkNumericOps(6).s)
    println(IntIsMyIntegral.plain(4).s)
    println(new StrIsMyNumeric.NumericOps("ab").s)
    val r = new Range0(1, 10, 3)(IntIsMyIntegral)
    println(List(r.next, r.prev, r.half))
    println(r.sum(3)(IntIsMyIntegral))
  }
}
