// The library half of the empty-parameter-list interoperability check.
//
// nsc distinguishes `def f: T` (a `NullaryMethodType`) from `def f(): T` (a
// `MethodType` over an empty list) and holds every call site to the
// difference. Compiled by scala-rs, this file is read back by real scalac
// (`pp_app.scala`); compiled by scalac it is the control.

case class Empty()

case class Pair(a: Int, b: Int)

trait Ticker {
  def tick(): Int
  def label: String
  def twice(): Int = tick() * 2
}

class Counter(start: Int) extends Ticker {
  private var n = start
  def tick(): Int = { n += 1; n }
  def label: String = "counter"
}

trait Louder extends Ticker {
  // A `super` call on a method with an empty parameter list: the super
  // accessor a stackable trait declares carries the accessed method's type,
  // clauses included.
  abstract override def tick(): Int = super.tick() + 100
}

class Loud(start: Int) extends Counter(start) with Louder

object Keys {
  def empty(): Empty = Empty()
  def nullary: Int = 7
  def unit(): Unit = ()
  def withImplicit()(implicit ord: Ordering[Int]): Int = ord.compare(1, 2)
  def curried(a: Int)(b: Int): Int = a + b
  def emptyThenValue()(b: Int): Int = b + 1
  def generic[A](a: A)(): List[A] = List(a)
  def deep()()(): String = "deep"
}
