// The legal neighbours of the rejections `rtp_neg_*` pins: none of these may
// start failing. An anonymous subclass of an abstract class or trait, a
// `private[this] var` of a covariant type, a field defaulted with `= _`, a
// `lazy` local that refers to itself, a subclass of a non-final class, and a
// `def x_=` beside a `private[this] var x`, which has no setter to clash with.
object Main {
  abstract class A { def f: Int }
  trait T { def g: Int = 2 }
  class Cell[+X](init: X) { private[this] var a: X = init; def get: X = a; val b: X = init }
  class Sink[-X] { private[this] var last: Any = null; def put(x: X): Unit = last = x; var count = 0 }
  class Fields { var i: Int = _; var s: String = _ }
  class Open; class Sub extends Open
  case class P(x: Int)
  class NotCase(x: Int) extends P(x)
  class Own { private[this] var x: Int = 0; def x_=(v: Int): Unit = x = v * 2; def get = x }
  def main(args: Array[String]): Unit = {
    val a = new A { def f = 1 }
    val t = new T {}
    println(a.f + t.g + new Cell(3).get + new Cell(4).b)
    val s = new Sink[String]; s.put("x"); s.count += 1; println(s.count)
    val fl = new Fields; println(fl.i + " " + fl.s)
    lazy val fibs: LazyList[BigInt] = BigInt(0) #:: BigInt(1) #:: fibs.zip(fibs.tail).map { case (x, y) => x + y }
    println(fibs.take(8).toList)
    def fact(n: Int): Int = if (n <= 1) 1 else n * fact(n - 1)
    println(fact(5) + " " + new Sub().isInstanceOf[Open] + " " + new NotCase(3).x)
    val o = new Own; o.x_=(21); println(o.get)
    println((1: Any).isInstanceOf[Int] + " " + ((2: Any) match { case _: Int => "int"; case _ => "?" }))
  }
}
