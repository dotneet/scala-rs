// The legal neighbours of the rejections `rtp_neg_*` pins: none of these may
// start failing. An anonymous subclass of an abstract class or trait, a
// `private[this] var` of a covariant type, a field defaulted with `= _`, a
// `lazy` local that refers to itself, and a subclass of a non-final class.
object Main {
  abstract class A { def f: Int }
  trait T { def g: Int = 2 }
  class Cell[+X](init: X) { private[this] var a: X = init; def get: X = a; val b: X = init }
  class Sink[-X] { private[this] var last: Any = null; def put(x: X): Unit = last = x; var count = 0 }
  class Fields { var i: Int = _; var s: String = _ }
  class Open; class Sub extends Open
  case class P(x: Int)
  class NotCase(x: Int) extends P(x)
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
    println((1: Any).isInstanceOf[Int] + " " + ((2: Any) match { case _: Int => "int"; case _ => "?" }))
  }
}
