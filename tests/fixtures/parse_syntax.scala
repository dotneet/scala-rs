// Expression and pattern syntax: implicit function literals in blocks and
// case bodies, `*` as an infix extractor, `(xs) @ _*`, a method value inside
// an infix chain, `+` as a method name, the reference zero of an abstract
// type, and `@strictfp`.

import scala.annotation.strictfp

object Star {
  case class *(a: Int, b: Int)
  case class P(a: Int, b: *)
}

abstract class Linked {
  type Node <: Node0
  abstract class Node0 {
    self: Node =>
    // `= _` is not typed against `Node` (`= null` would be a mismatch).
    var next: Node = _
    def isLast = next == null
  }
}

class Cell[A] {
  private var value: A = _
  def set(a: A): this.type = { value = a; this }
  def get: A = value
}

// A constructor default may name the companion's private members
// (`pos/t5217`); the `private` on the line after `class B` belongs to `b`.
object Comp {
  private class B { override def toString = "B" }
  private val b: B = new B
  def make(): Comp = new Comp()
}
class Comp private (b: Comp.B = Comp.b) {
  override def toString = "Comp(" + b + ")"
}

@strictfp class Strict {
  def twice(x: Double) = x * 2
}

object Main {
  def foo(f: Int => Int) = f(10)
  def twice(x: Int) = x * 2
  def inc(x: Int) = x + 1
  def +[T](x: T): String = "plus " + x

  def main(args: Array[String]): Unit = {
    // nsc `prefixExpr`: `-` in front of a numeric literal belongs to the
    // literal, so the selections after it apply to the negative number.
    println(-3.0.abs)
    println(-3.abs)
    println(- 3.0.abs)
    println(-1.toString)
    println(-0x10.abs)
    println(-1.5e10.toLong)
    // `MinValue` exists only as a negated literal.
    println(-2147483648)
    println((-2147483648).abs)
    println(-9223372036854775808L)
    println(0x80000000)
    println(-0x80000000)
    val five = 5
    println(-five.abs)
    println(-3.0 max 2.0)
    println(foo { implicit x => x + 1 })
    println(foo { implicit x: Int => implicitly[Int] * 2 })
    println(foo { implicit _ => 42 })
    println(foo { implicit _: Int => implicitly[Int] + 3 })
    println(foo(implicit x => x + 4))
    println(foo(implicit (x: Int) => x + 5))
    val fs = List(1, -1) map {
      case i if i > 0 => implicit j: Int => i + implicitly[Int]
      case _ => implicit j: Int => 42
    }
    println(fs.map(_(100)))

    import Star._
    val x * y = new *(6, 7)
    println(s"$x $y")
    val P(a, b * c) = P(5, new *(8, 9))
    println(s"$a $b $c")
    Seq(1, 2, 3) match {
      case Seq((xs) @ _*) => println(xs)
    }

    // `(twice _).compose(...)`, not `(twice _ compose ...) _`.
    val h = twice _ compose ((x: Int) => x + 1)
    println(h(4))
    println((twice _ andThen ((x: Int) => x + 1))(4))

    println(`+`[Int](6))
    println(`+`(7))
    println(+(6))

    println(new Cell[String].get)
    println(new Cell[Int].set(3).get + 1)
    println(new Cell[String].set("s").get)

    val strict = classOf[Strict].getDeclaredMethods.toList
      .filter(m => java.lang.reflect.Modifier.isStrict(m.getModifiers)).map(_.getName)
    println(strict)
    println(new Strict().twice(1.5))
    println(Comp.make())
  }
}
