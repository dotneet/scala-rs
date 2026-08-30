// Erasure has to box the value of a *block* exactly once when it leaves a
// method whose erased result is `Object`. A `Block`, `If`, `Match` and `Try`
// hand the expected type straight to the subexpressions that produce their
// value, so those are boxed already; boxing the node again emitted
// `boxToInteger(boxToInteger(z))` in `Main$$anon$1.next()Ljava/lang/Object;`
// and the class did not verify.

class Cell[A](val a: A) {
  override def toString: String = "Cell(" + a + ")"
}

trait It[A] { def next(): A }

trait F[A] { def f(a: A): Int }

trait P[A, B] { def one(): A; def two(): B }

abstract class Ab[A] { def get(): A }

trait V[A] { def value: A }

class NamedInt extends It[Int] { def next(): Int = { val z = 3; z } }

class NamedAb extends Ab[Long] { def get(): Long = { val z = 4L; z } }

object Main {
  def id[A](x: A): A = x

  def main(args: Array[String]): Unit = {
    // Every primitive, block body, anonymous implementation of a generic trait.
    println(new It[Int] { def next(): Int = { val z = 1; z } }.next())
    println(new It[Long] { def next(): Long = { val z = 2L; z } }.next())
    println(new It[Double] { def next(): Double = { val z = 1.5; z } }.next())
    println(new It[Float] { def next(): Float = { val z = 2.5f; z } }.next())
    println(new It[Boolean] { def next(): Boolean = { val z = true; z } }.next())
    println(new It[Char] { def next(): Char = { val z = 'x'; z } }.next())
    println(new It[Byte] { def next(): Byte = { val z: Byte = 7; z } }.next())
    println(new It[Short] { def next(): Short = { val z: Short = 8; z } }.next())

    // `Unit` is deliberately absent: nothing is boxed there (`()` never
    // reaches a reference position), and calling a `Unit` member whose erased
    // result is `Object` leaves the returned value on the stack -- a separate,
    // pre-existing defect in the `Unit` erasure, not this one.

    // Expression body -- the shape that already worked, kept as a control.
    println(new It[Int] { def next(): Int = 9 }.next())
    // Reference type -- no boxing owed at all.
    println(new It[String] { def next(): String = { val z = "s"; z } }.next())

    // An abstract class, and named classes doing the same thing.
    println(new Ab[Int] { def get(): Int = { val z = 10; z } }.get())
    val n: It[Int] = new NamedInt
    println(n.next())
    println(new NamedInt().next())
    val na: Ab[Long] = new NamedAb
    println(na.get())

    // A primitive *parameter* as well as a primitive result.
    println(new F[Int] { def f(x: Int): Int = { val z = x + 1; z } }.f(41))

    // More than one type parameter, and a generic instantiated at a class.
    val p = new P[Int, Double] {
      def one(): Int = { val z = 11; z }
      def two(): Double = { val z = 12.5; z }
    }
    println(p.one())
    println(p.two())
    println(new It[Cell[Int]] { def next(): Cell[Int] = { val z = new Cell(13); z } }.next())

    // A `val` implementing an abstract member of the erased type.
    println(new V[Int] { val value: Int = { val z = 14; z } }.value)

    // SAM conversion of a lambda with a block body.
    val sam: It[Int] = () => { val z = 15; z }
    println(sam.next())

    // `while`, `if`, `match` and `try` bodies.
    println(new It[Int] {
      def next(): Int = { var k = 0; while (k < 5) { k += 1 }; k }
    }.next())
    println(new It[Int] { def next(): Int = if (1 < 2) 16 else 17 }.next())
    println(new It[Int] { def next(): Int = 3 match { case 3 => 18; case _ => 19 } }.next())
    println(new It[Int] {
      def next(): Int = try { val z = 20; z } catch { case _: Throwable => 0 }
    }.next())

    // A captured `var` mutated in the body.
    var c = 0
    val inc = new It[Int] { def next(): Int = { c += 1; c } }
    println(inc.next())
    println(inc.next())

    // The same double boxing outside any class: a block, an `if`, a `match`
    // and a `try` reaching an `Any` position, and a generic method argument.
    val a1: Any = { val z = 21; z }
    println(a1)
    val a2: Any = if (1 < 2) { val z = 22; z } else 23
    println(a2)
    val a3: Any = 1 match { case 1 => { val z = 24; z }; case _ => 25 }
    println(a3)
    val a4: Any = try { val z = 26; z } catch { case _: Throwable => 27 }
    println(a4)
    println(id({ val z = 28; z }))

    // The other direction: a reference-typed block reaching a primitive
    // position has to be unboxed exactly once too.
    val b: Int = { val z: Any = 29; z.asInstanceOf[Int] }
    println(b)
  }
}
