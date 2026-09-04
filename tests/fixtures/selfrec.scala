// Fixture for the `agent/selfrec` slice. Every case here is one that stopped
// `tests/slick_run.sh`; the expectation is real scalac 2.13.16's own stdout for
// this file (tests/fixtures/expected/selfrec.txt).
//
// 1. A curried call that omits a default argument in a clause that is *not*
//    the last one, on a receiver that has to be evaluated (an `Apply`, a
//    `new`, ...). The receiver is hoisted into a local so it runs once, and
//    the hoist has to wrap the *whole* application chain: wrapping only the
//    clause that carries the default leaves `Apply { fun: Block, ... }`, which
//    has no callee for the backend to emit.
//
// 2. `asInstanceOf` / `isInstanceOf` whose qualifier is a JVM primitive. The
//    emitters read their receiver as an `Object`, so a primitive has to be
//    boxed first -- or, when the target is a primitive too, turned into a
//    numeric conversion, which is what nsc's erasure does.

trait Session { def id: String }

// Receiver evaluations are counted so the hoist is checked, not just the
// bytecode's shape.
object Count { var recv = 0 }

trait Invoker[+R] {
  def rows(maxRows: Int): List[R @scala.annotation.unchecked.uncheckedVariance]

  // slick's `Invoker.foreach`: a default in the first clause, an implicit
  // second clause the call site applies explicitly.
  final def foreach(f: R => Unit, maxRows: Int = 0)(implicit s: Session): Unit = {
    println("foreach on " + s.id + " maxRows=" + maxRows)
    rows(maxRows).foreach(f)
  }
  // The same shape with a plain second clause and no function parameter.
  final def take(n: Int, maxRows: Int = 0)(s: Session): List[R] =
    rows(maxRows).take(n)
  // Three clauses, the default in the middle one.
  final def slice(from: Int)(n: Int = 2)(s: Session): List[R] =
    rows(0).drop(from).take(n)
}

class IntInvoker extends Invoker[Int] {
  def rows(maxRows: Int): List[Int] =
    if (maxRows == 0) List(1, 2, 3, 4) else List(1)
}

class Single[T](val v: T) {
  override def toString = "Single(" + v + ")"
}

abstract class Casts[R] {
  def toAbstract(i: Int): Any = new Single[R](i.asInstanceOf[R])
  def boolToAbstract(b: Boolean): Any = new Single[R](b.asInstanceOf[R])
  def toAny(i: Int): Any = i.asInstanceOf[Any]
  def toAnyRef(i: Int): Any = i.asInstanceOf[AnyRef]
  def toBoxed(i: Int): Integer = i.asInstanceOf[Integer]
  def toSame(i: Int): Int = i.asInstanceOf[Int]
  def toLong(i: Int): Long = i.asInstanceOf[Long]
  def toByte(i: Int): Byte = i.asInstanceOf[Byte]
  def toDouble(l: Long): Double = l.asInstanceOf[Double]
  def fromAny(a: Any): Int = a.asInstanceOf[Int]
  def primIsRef(i: Int): Boolean = i.isInstanceOf[Any]
  def refIsPrim(a: Any): Boolean = a.isInstanceOf[Int]
}

object Main {
  implicit val session: Session = new Session { def id = "s1" }

  def mk(): Invoker[Int] = { Count.recv += 1; new IntInvoker }

  def main(args: Array[String]): Unit = {
    val b = List.newBuilder[Int]
    // The shape that stopped every `.result` in `slick_run.sh`.
    mk().foreach(x => b += x)(session)
    println(b.result())
    println("recv=" + Count.recv)

    Count.recv = 0
    println(mk().take(2)(session))
    println("recv=" + Count.recv)

    Count.recv = 0
    println(mk().slice(1)()(session))
    println("recv=" + Count.recv)

    // A `new` receiver takes the same path.
    println((new IntInvoker).take(3)(session))
    // A defaulted argument written out, and a stable receiver, still work.
    val inv: Invoker[Int] = new IntInvoker
    println(inv.take(2, 0)(session))
    inv.foreach(x => print(x + " "))
    println()

    val c = new Casts[Any] {}
    println(c.toAbstract(3))
    println(c.boolToAbstract(true))
    println(c.toAny(4))
    println(c.toAnyRef(5))
    println(c.toBoxed(6))
    println(c.toSame(7))
    println(c.toLong(8))
    println(c.toByte(300))
    println(c.toDouble(9L))
    println(c.fromAny(10))
    println(c.primIsRef(11))
    println(c.refIsPrim(12))
    println(c.refIsPrim("no"))
  }
}
