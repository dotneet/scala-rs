// Regression test for the bug where an implicit argument clause stayed unapplied
// in the type of the expression. Four roots in one file (real scalac 2.13.16
// produces the same output).

import scala.collection.Factory
import scala.reflect.ClassTag

// (1) Line the *result* of a function parameter up with the parameter's own class
//     before solving the type arguments. Given `flatMap[B](f: A => IterableOnce[B])`
//     a lambda whose body returns `Map[K, V]`, we used to zip `[B]` against `[K, V]`
//     positionally and solve `B = K`. `toMap` then found no `A <:< (K, V)` and
//     `(<:<[K, (K, V)])Map[K, V]` was left as the type of the expression.
object Root1 {
  def collect(mapped: Vector[(String, Map[Long, Int])]): Map[Long, Int] =
    mapped.iterator.flatMap(_._2).toMap
  def isEmptyOf(mapped: Vector[(String, Map[Long, Int])]): Boolean =
    mapped.iterator.flatMap(_._2).toMap.isEmpty
}

// (2) Passing a class that *inherits* `A => B` (`<:<` is one) to a Function1
//     parameter did not solve the callee's type arguments from that argument.
//     Conformance itself goes through, so `val g: R => S = ev` is writable, yet
//     `flatMap(ev)` said "no matching overload".
abstract class Conv[-A, +B] extends (A => B)
final class Act[+R](val value: R) {
  def flatMap[R2](f: R => Act[R2]): Act[R2] = f(value)
  def flatten[R2](implicit ev: R <:< Act[R2]): Act[R2] = flatMap(ev)
}
object Root2 {
  def viaConv[A, B](ev: Conv[A, B], a: A): B = {
    def id[X, Y](f: X => Y): X => Y = f
    id(ev)(a)
  }
  val upper: Conv[String, String] = new Conv[String, String] {
    def apply(s: String) = s.toUpperCase
  }
}

// (3) The *qualifier* of a selection gets its implicit clause filled in before use,
//     even inside a call argument. The `pack` of `SV(pack.to[Seq], "x")` was left
//     as `(Sh[…])Qy[R]`, and `to` came out "not a member".
trait Sh[-M, P]
final case class SV[A, B](value: A, shape: B)
final class Qy[E](val name: String) {
  def pack[R](implicit packing: Sh[E, R]): Qy[R] = new Qy[R](name + "+p")
  def to[D[_]]: Qy[E] = new Qy[E](name + "+t")
}
object Root3 {
  implicit def idSh[T]: Sh[T, T] = new Sh[T, T] {}
  def wrap(q: Qy[Int]): SV[Qy[Int], String] = SV(q.pack.to[Seq], "x")
}

// (4) When a derivation rule carries a `ClassTag` in its own implicit arguments the
//     rule was dropped as an "unusable candidate". A `ClassTag` is filled by
//     synthesis rather than search, and `implicitly[ClassTag[Seq[Any]]]` alone did pass.
trait Coll[C[_]] { def name: String }
object Coll {
  implicit def forColl[C[X] <: Iterable[X]](implicit
      cbf: Factory[Any, C[Any]],
      tag: ClassTag[C[Any]]
  ): Coll[C] = new Coll[C] { def name = "coll" }
}

object Main {
  def main(args: Array[String]): Unit = {
    val v = Vector(("a", Map(1L -> 10)), ("b", Map(2L -> 20)))
    println(Root1.collect(v).toSeq.sortBy(_._1))
    println(Root1.isEmptyOf(v))
    println(Root1.isEmptyOf(Vector()))
    println(Root2.viaConv(Root2.upper, "hi"))
    println(new Act(new Act(7)).flatten.value)
    println(Root3.wrap(new Qy[Int]("q")).value.name)
    println(implicitly[Coll[Vector]].name)
    println(implicitly[Coll[Seq]].name)
  }
}
