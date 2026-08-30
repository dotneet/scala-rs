// `Unit` members reached through a trait, an abstract class and a value
// class: the interface method, the mixin forwarder, the `T$class` static
// implementation and the erasure bridge all have to agree that the parameter
// is a `scala/runtime/BoxedUnit`, and that the abstract `var`'s setter takes
// one.
trait HasU {
  def u: Unit
  var mu: Unit
  def useU(x: Unit): String
  def defaultU(x: Unit): String = "d"
}

trait Mixin extends HasU {
  val tv: Unit = ()
  override def useU(x: Unit): String = "m"
}

class Impl extends Mixin {
  def u: Unit = ()
  var mu: Unit = ()
}

abstract class Base {
  def take(u: Unit): String
}

class Sub extends Base {
  def take(u: Unit): String = "sub"
}

class VC(val x: Int) extends AnyVal {
  def plus(u: Unit, n: Int): Int = x + n
}

object Main {
  def main(args: Array[String]): Unit = {
    val i = new Impl
    println(i.u)
    println(i.mu)
    i.mu = ()
    println(i.mu)
    println(i.tv)
    println(i.useU(()))
    println(i.defaultU(()))
    val h: HasU = i
    println(h.useU(()))
    println(h.defaultU(()))
    println(h.u)
    val b: Base = new Sub
    println(b.take(()))
    println(new VC(1).plus((), 2))
    val fn: Int => Unit = n => ()
    println(fn(3))
  }
}
