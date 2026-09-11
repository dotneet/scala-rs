// Value classes and function values at the edges of their representation:
// a library value class's member inherited from a trait runs on a box
// (`RichInt.signum` is `ScalaNumberProxy`'s), a lambda parameter typed at a
// user value class arrives boxed and has to be unboxed for an `$extension`
// call, and applying a `def` that returns a `Unit` function drops its result
// once.
object Main {
  case class Id(raw: Int) extends AnyVal { def next: Id = Id(raw + 1) }
  class Meter(val v: Double) extends AnyVal { def scale(k: Int): Meter = new Meter(v * k); override def toString = s"${v}m" }
  class A { var n = 0; def adder: Int => Unit = k => n += k; val f: Int => Unit = k => n += k * 10 }
  def main(args: Array[String]): Unit = {
    println(5.signum + " " + (-5).signum + " " + 0.signum + " " + 5L.signum + " " + 2.5f.signum + " " + 'a'.signum)
    println(5.0.isWhole + " " + 5.5.isWhole + " " + 300.isValidByte + " " + 100.isValidByte + " " + 70000L.isValidInt + " " + 97.isValidChar)
    val x = 42
    println(x.signum * 2 + x.sign + " " + 5.max(3) + " " + 5.abs)
    println(List(Id(3), Id(1)).map(_.next) + " " + List(Id(3), Id(1)).sortBy(_.raw).map(_.next.raw))
    val g: Id => Id = _.next
    println(g(Id(0)) + " " + Option(new Meter(1.5)).map(_.scale(2)))
    val a = new A
    a.adder(100)
    a.f(1)
    val h = a.adder
    h(1000)
    println(a.n)
  }
}
