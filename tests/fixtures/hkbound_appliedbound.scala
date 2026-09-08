// `agent/hkbound`: an *applied* higher-kinded type parameter is at least its
// own bound, applied to the same arguments, and `@uncheckedVariance` is a
// spelling rather than a type.
//
// Every line prints, and the printed value is what a wrong conformance answer
// would change: `choose` selects between two overloads, one of which is only
// applicable because `CC[K0, V0]` conforms to the parameter's own bound. On
// the pre-fix binary the first three groups do not compile at all and the
// fourth compiles to the *other* overload.
import scala.annotation.unchecked.uncheckedVariance

trait MapOps[K, V, +CC[_, _], +C] {
  def factory: Fac[CC]
  def opsTag: String = "MapOps"
}

trait Fac[+CC[_, _]] {
  def build[K, V](k: K, v: V): CC[K, V]
}

class MyMap[K, V](val k: K, val v: V) extends MapOps[K, V, MyMap, MyMap[K, V]] {
  def factory: Fac[MyMap] = MyMap
  override def toString: String = "MyMap(" + k.toString + "," + v.toString + ")"
}

object MyMap extends Fac[MyMap] {
  def build[K, V](k: K, v: V): MyMap[K, V] = new MyMap(k, v)
}

// -- the shape `BuildFrom.scala` is written in --------------------------------

object Build {
  // The ascription is exactly `CC`'s own bound, so it holds by the bound and
  // by nothing else. This is `BuildFrom.buildFromMapOps#newBuilder`.
  def rebuild[K0, V0, K, V, CC[X, Y] <: MapOps[X, Y, CC, _]](
      from: CC[K0, V0]
  )(k: K, v: V): CC[K, V] =
    (from: MapOps[K0, V0, CC, _]).factory.build(k, v)

  // The same conformance in an argument position rather than an ascription.
  // The type arguments are written out: inferring `CC` *from* an argument
  // whose type only reaches the formal through the bound is a separate
  // mechanism (`base_type_args` through a type parameter's bound) and this
  // slice does not touch it.
  def tagOf[K0, V0, CC[X, Y] <: MapOps[X, Y, CC, _]](from: CC[K0, V0]): String =
    readTag[K0, V0, CC](from)

  def readTag[K0, V0, CC[X, Y] <: MapOps[X, Y, CC, _]](m: MapOps[K0, V0, CC, _]): String =
    m.opsTag

  // Overload selection, which a conformance rule that reads a bound too
  // eagerly silently changes. The applicable alternative is the one that is
  // only applicable *through the bound*, so this line is what a wrong
  // conformance answer would rewrite rather than reject: scalac 2.13.16 runs
  // `pick:ops`, and so must we.
  //
  // The type arguments are written out here for the same reason as `tagOf`
  // above. With them inferred, `pick(from)` still selects the `Any`
  // alternative -- a silent wrong answer that this slice does *not* fix and
  // did not introduce, recorded in `docs/scala-library.md`.
  def pick[K0, V0, CC[X, Y] <: MapOps[X, Y, CC, _]](x: MapOps[K0, V0, CC, _]): String =
    "pick:ops"
  def pick(x: Any): String = "pick:any"

  def choose[K0, V0, CC[X, Y] <: MapOps[X, Y, CC, _]](from: CC[K0, V0]): String =
    pick[K0, V0, CC](from)
}

// -- the shape `Factory.scala`'s `fill` ladder is written in ------------------

class Box[A](val a: A) {
  override def toString: String = "Box(" + a.toString + ")"
}

trait Nest[+CC[_]] {
  def one[A](a: A): CC[A]
  // `found: CC[CC[A]]  required: CC[CC[A] @uncheckedVariance]`
  def two[A](a: A): CC[CC[A] @uncheckedVariance] = one(one(a))
  // `found: CC[CC[CC[A] @uncheckedVariance]]  required: CC[CC[CC[A]] @uncheckedVariance]`
  // -- the annotation sits at a different depth on each side.
  def three[A](a: A): CC[CC[CC[A]] @uncheckedVariance] = one(two(a))
}

object BoxNest extends Nest[Box] {
  def one[A](a: A): Box[A] = new Box(a)
  // The other direction: `found: CC[CC[A] @uncheckedVariance] required: CC[CC[A]]`.
  def twoPlain[A](a: A): Box[Box[A]] = two(a)
}

object Main {
  def main(args: Array[String]): Unit = {
    val m = new MyMap[Int, String](1, "one")
    println(Build.rebuild[Int, String, String, Int, MyMap](m)("two", 2).toString)
    println(Build.tagOf[Int, String, MyMap](m))
    println(Build.choose[Int, String, MyMap](m))
    println(BoxNest.two(7).toString)
    println(BoxNest.three(7).toString)
    println(BoxNest.twoPlain(7).toString)
  }
}
