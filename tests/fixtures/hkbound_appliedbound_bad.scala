// `agent/hkbound`: the restrictions on reading an applied higher-kinded
// parameter's bound. Real scalac 2.13.16 rejects every line below, and so must
// we -- reading the bound is what makes the positive fixture compile, and
// reading it too eagerly is what would make unrelated types conform.
//
// Every one of these was already rejected before the fix. The fixture is what
// stops the new arms from over-reaching, not evidence that they exist.
import scala.annotation.unchecked.uncheckedVariance

trait MapOps[K, V, +CC[_, _], +C] {
  def opsTag: String = "MapOps"
}

trait Sink[K, V] {
  def sinkTag: String = "Sink"
}

class Box[A](val a: A)

object Bad {
  // 1. The bound is `MapOps`, and `Sink` is not it. A rule that reduced an
  //    applied abstract constructor to *anything* would accept this.
  def notTheBound[K0, V0, CC[X, Y] <: MapOps[X, Y, CC, _]](from: CC[K0, V0]): String =
    (from: Sink[K0, V0]).sinkTag

  // 2. Two F-bounded parameters with the *same* bound shape are still two
  //    different constructors: `CC[Int, String]` is not a `DD[Int, String]`.
  def notEachOther[
      CC[X, Y] <: MapOps[X, Y, CC, _],
      DD[X, Y] <: MapOps[X, Y, DD, _]
  ](x: CC[Int, String]): DD[Int, String] = x

  // 3. The reduction is not symmetric. A `CC[K0, V0]` is a `MapOps[...]`;
  //    a `MapOps[...]` is not a `CC[K0, V0]`.
  def notReverse[K0, V0, CC[X, Y] <: MapOps[X, Y, CC, _]](
      m: MapOps[K0, V0, CC, _]
  ): CC[K0, V0] = m

  // 4. Erasing `@uncheckedVariance` for conformance does not erase the type
  //    under it: a `Box[A]` is still not a `Box[String]`.
  def annotationIsNotAWildcard[A](b: Box[A] @uncheckedVariance): Box[String] = b
}
