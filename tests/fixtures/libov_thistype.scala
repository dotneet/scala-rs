// `this.type` results (SLS 3.2.1): a call to a member declared `: this.type`
// on a stable prefix `p` has type `p.type`; on `super` it is this class's
// `this.type`; on a receiver typed by an abstract type `C` it is a `C`.
// scala/scala's own collections are written this way (`Growable`, `Buffer`,
// `Stack`, every builder's `super.addAll`).
trait Growable[-A] {
  def addOne(elem: A): this.type
  @inline final def +=(elem: A): this.type = addOne(elem)
  def addAll(xs: List[A]): this.type = { xs.foreach(addOne); this }
  @inline final def ++=(xs: List[A]): this.type = addAll(xs)
}

trait Ops[A, C <: Ops[A, C]] extends Growable[A] {
  def empty: C
  def cl(): C
  // `cl()` is not stable, so `+=` on it is a `C`, not an `Ops[A, C]`.
  final def plusOne(a: A): C = cl() += a
  def copied(xs: List[A]): C = empty ++= xs
}

abstract class Buf[A] extends Growable[A] {
  var elems: List[A] = Nil
  def addOne(elem: A): this.type = { elems = elem :: elems; this }
  // Inherited members, unqualified and through `this`.
  final def append(elem: A): this.type = addOne(elem)
  def appendAll(xs: List[A]): this.type = addAll(xs)
  def viaThis(a: A): this.type = this += a
  def chain(a: A, b: A): this.type = this += a += b ++= Nil
  def cond(b: Boolean, a: A): this.type = if (b) this else append(a)
}

class Buf2[A] extends Buf[A] with Ops[A, Buf2[A]] {
  def empty: Buf2[A] = new Buf2[A]
  def cl(): Buf2[A] = { val c = new Buf2[A]; c.elems = elems; c }
  // `super.addAll` is `Buf2.this.type`; the JVM result is `Growable`, so the
  // backend has to cast it back.
  override def addAll(xs: List[A]): this.type = xs match {
    case Nil => this
    case _   => super.addAll(xs)
  }
  // A `var` inferred from a `this.type` call is widened (nsc
  // `widenIfNecessary`), so it can be reassigned.
  def swapped(other: Buf2[A]): Buf2[A] = {
    var cur = append(elems.head)
    cur = other
    cur
  }
}

class Pair[A](val x: A) {
  def self: this.type = this
  // A `this.type` nested in the result, read through `super`.
  def both: (this.type, A) = (this, x)
}
class Pair2[A](x: A) extends Pair[A](x) {
  override def both: (this.type, A) = super.both
}

object Main {
  // Stable prefixes: a parameter, followed through a chain of calls.
  def twice[A](b: Buf[A], x: A): b.type = b.addOne(x).addOne(x)
  def appended[A](b: Buf[A], x: A): b.type = b.append(x) += x
  def main(args: Array[String]): Unit = {
    val b = new Buf2[Int]
    twice(b, 1)
    appended(b, 2)
    b.viaThis(3).chain(4, 5).cond(false, 6).appendAll(List(7))
    // A dependent result used as a receiver: `twice(b, 8)` is a `b.type`,
    // whose `append` takes an `Int`.
    twice(b, 8).append(9)
    println(b.elems)
    println(b.plusOne(10).elems.head)
    println(b.copied(List(11, 12)).elems)
    println(b.swapped(new Buf2[Int]).elems)
    val p = new Pair2("p")
    println((p.self eq p, p.both._1 eq p, p.both._2))
  }
}
