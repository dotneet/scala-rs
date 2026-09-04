// agent/kernel: ten things cats-kernel writes that this compiler rejected.
// All ten are plain Scala 2.13 -- no compiler plugin involved, and real
// scalac 2.13.16 accepts this file (`crates/cli/tests/kernel.rs` runs both).
//
//  1. A higher-kinded type parameter's bound is written in its *own*
//     parameters: `class Fns[P[T] <: Eq0[T]]`. `P[A]` is an `Eq0[A]`.
//  2. `Tuple1[A]` is writable by name even though no surface syntax builds one.
//  3. A class whose only clause is implicit has the constructor
//     `()(implicit ...)`, so `extends E[K, V]()(V)` and `this()(V)` are calls
//     with two argument lists.
//  4. `FiniteDuration` overloads `min` / `max` / `+` / `-` over `Duration`'s,
//     and the more specific alternative wins.
//  5. `trait StaticAnnotation extends Annotation`, so it can be mixed in.
//  6. `immutable.BitSet` is a `SortedSet[Int]`, hence a `Set[Int]`.
//  7. `new BigDecimal(java.math.BigDecimal, java.math.MathContext)`.
//  8. A `{ case ... }` literal takes as many parameters as the expected SAM's
//     single abstract method does.
//  9. A hexadecimal literal spans the unsigned range of its type and is read
//     as two's complement: `0x85ebca6b` is an `Int`.
// 10. `Duration.MinusInf` has type `Duration.Infinite`, which is a `Duration`.

import java.math.MathContext
import scala.annotation.{Annotation, StaticAnnotation}
import scala.collection.immutable.BitSet
import scala.concurrent.duration.{Duration, FiniteDuration, SECONDS}

object Main {
  // 1.
  trait Eq0[T] {
    def eqv(x: T, y: T): Boolean
    def self: T
  }
  abstract class Fns[P[T] <: Eq0[T]] {
    def eqv[A](x: A, y: A)(implicit ev: P[A]): Boolean = ev.eqv(x, y)
    def mk[A](implicit ev: P[A]): A = ev.self
  }
  class IntEq extends Eq0[Int] {
    def eqv(x: Int, y: Int): Boolean = x == y
    def self: Int = 7
  }
  object Fns extends Fns[Eq0]

  // 2.
  def firstOf[A](t: Tuple1[A]): A = t._1

  // 3.
  class E[K, V](implicit V: Eq0[V]) {
    def witness: Eq0[V] = V
    def this(V: Eq0[V], unused: Int) = this()(V)
  }
  class H[K, V](implicit V: Eq0[V], K: Eq0[K]) extends E[K, V]()(V) {
    def this(V: Eq0[V], unused: Int, K: Eq0[K]) = this()(V, K)
  }

  // 4.
  def durMin(x: FiniteDuration, y: FiniteDuration): FiniteDuration = x.min(y)
  def durMax(x: FiniteDuration, y: FiniteDuration): FiniteDuration = x.max(y)
  def durAdd(x: FiniteDuration, y: FiniteDuration): FiniteDuration = x + y
  def durSub(x: FiniteDuration, y: FiniteDuration): FiniteDuration = x - y

  // 5.
  class marker extends Annotation with StaticAnnotation
  class staticOnly extends StaticAnnotation

  // 6.
  def subset(x: BitSet, y: BitSet): Boolean = x.subsetOf(y)
  def union(x: BitSet, y: BitSet): BitSet = x | y
  def asSet(x: BitSet): Set[Int] = x

  // 7.
  def bdAdd(x: BigDecimal, y: BigDecimal): BigDecimal =
    new BigDecimal(x.bigDecimal.add(y.bigDecimal), x.mc)
  def bdCopy(x: BigDecimal): BigDecimal = new BigDecimal(x.bigDecimal)
  def mathContext(x: BigDecimal): MathContext = x.mc

  // 8. `Pred2` has one abstract method, so a `{ case ... }` literal for it
  //    takes two parameters and matches the pair. Only the abstract method is
  //    called: a trait's concrete members are not reachable through a SAM
  //    instance yet (see `docs/cats.md`).
  trait Pred2[A] {
    def test(x: A, y: A): Boolean
    def negate(x: A, y: A): Boolean = !test(x, y)
  }
  val pairEq: Pred2[Option[Int]] = {
    case (Some(a), Some(b)) => a == b
    case (None, None)       => true
    case _                  => false
  }

  // 9.
  def avalanche(hash: Int): Int = {
    var h = hash
    h ^= h >>> 16
    h *= 0x85ebca6b
    h ^= h >>> 13
    h *= 0xc2b2ae35
    h ^= h >>> 16
    h
  }

  // 10.
  def lowest: Duration = Duration.MinusInf
  def highest: Duration = Duration.Inf

  def main(args: Array[String]): Unit = {
    implicit val intEq: Eq0[Int] = new IntEq
    println(Fns.eqv(3, 3))
    println(Fns.mk[Int])
    println(firstOf(Tuple1("one")))
    println(new H[Int, Int]().witness.self)
    // `new`, not `FiniteDuration(2L, SECONDS)`: the companion's `apply` is a
    // known gap once `Duration.Infinite` has been read (see `docs/cats.md`).
    val a = new FiniteDuration(2L, SECONDS)
    val b = new FiniteDuration(5L, SECONDS)
    println(durMin(a, b))
    println(durMax(a, b))
    println(durAdd(a, b))
    println(durSub(b, a))
    println(new marker().getClass.getName)
    println(new staticOnly().getClass.getName)
    val s1 = BitSet(1, 2, 3)
    val s2 = BitSet(1, 2)
    println(subset(s2, s1))
    println(union(s1, s2))
    println(asSet(s2).size)
    println(bdAdd(BigDecimal(2), BigDecimal(3)))
    println(bdCopy(BigDecimal(4)))
    println(mathContext(BigDecimal(4)).getPrecision)
    println(pairEq.test(Some(1), Some(1)))
    println(pairEq.test(Some(1), Some(2)))
    println(pairEq.test(None, None))
    println(avalanche(7))
    println(0xffffffff)
    println(0xffffffffffffffffL)
    println(lowest)
    println(highest)
  }
}
