// Recursive type definitions that are legal and must stay legal after cycle
// detection went in. Every one of these is accepted by real scalac 2.13.16.
//
//  * F-bounded polymorphism: the bound names the parameter it bounds, as an
//    *argument* of a class. `Ord[A <: Ord[A]]` is the textbook case.
//  * An abstract type bounded by a class applied to itself (`type E <: List[E]`).
//  * A type member of an enclosing class reached through `Outer.this.T`,
//    which is a different symbol from the inner member of the same name.
//  * A value class over a primitive, over a reference, and over an
//    unbounded type parameter -- the shapes the "may not wrap another
//    user-defined value class" rule must not touch.
object Main {
  trait Ord[A <: Ord[A]] { self: A =>
    def cmp(other: A): Int
    def below(other: A): Boolean = cmp(other) < 0
  }
  class Nat(val n: Int) extends Ord[Nat] {
    def cmp(other: Nat): Int = n - other.n
  }

  trait Nest {
    type E <: List[E]
    def depth(e: E): Int
  }
  class Nest0 extends Nest {
    type E = Nil.type
    def depth(e: E): Int = e.length
  }

  trait Outer {
    type T
    def one: T
    trait Inner {
      type T <: Outer.this.T
      def widen(t: T): Outer.this.T = t
    }
  }
  class OuterInt extends Outer {
    type T = Int
    def one: Int = 1
    class InnerInt extends Inner {
      type T = Int
    }
  }

  class Meters(val v: Int) extends AnyVal {
    def +(o: Meters): Meters = new Meters(v + o.v)
  }
  class Tag(val s: String) extends AnyVal
  class Box[A](val a: A) extends AnyVal

  def main(args: Array[String]): Unit = {
    println(new Nat(1).below(new Nat(2)))
    println(new Nat(3).below(new Nat(2)))
    println(new Nest0().depth(Nil))
    val o = new OuterInt
    println(o.one)
    println(new o.InnerInt().widen(7))
    println((new Meters(2) + new Meters(3)).v)
    println(new Tag("hi").s)
    println(new Box[Int](4).a)
  }
}
