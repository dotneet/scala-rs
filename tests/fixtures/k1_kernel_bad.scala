// The other side of `k1_kernel.scala`: shapes real scalac 2.13.16 rejects, so
// the fixes there did not simply start accepting everything.
//
// 1. `Annotation` is still a *class*; only `StaticAnnotation` became a trait,
//    so it cannot be the second parent.
// 2. `Tuple1[A]` really is a one-tuple, so `_1` has the element's type and not
//    the tuple's.
// 3. `BitSet` is a `Set[Int]`, not a `Set[String]`.
import scala.annotation.{Annotation, StaticAnnotation}
import scala.collection.immutable.BitSet

object Bad {
  class both extends StaticAnnotation with Annotation

  def firstOf(t: Tuple1[Int]): String = t._1

  def widen(x: BitSet): Set[String] = x
}
