// Shapes typelevel/cats' generated `NTuple*Instances` write. None of them
// needs a compiler plugin: kind-projector's `(A0, *, *)` desugars to the
// structural type lambda spelled out here, and scalac 2.13.16 accepts this
// file as it stands.
//
// 1. `copy` on a tuple. Every `TupleN` is a `case class` in scala-library, and
//    the prelude did not say so, so `(a, b).copy(_1 = x)` reported
//    `value copy is not a member of (Any, Any)`.
// 2. A member selected through a *fully applied* type lambda. The receiver
//    stayed the unreduced application, so `fa._2` reported
//    `value _2 is not a member of [x, y](A0, x, y)[A0, Any, Any]`.
// 3. A higher-kinded parameter bounded by a proper type (`F[_, _] <: Product`)
//    instantiated at a type lambda. nsc keeps the bound inside the `PolyType`,
//    so the comparison is `[x, y](A0, x, y) <: [x, y]Product`, decided on the
//    bodies.
// 4. A method type parameter solved from a *compound* expected type
//    (`Trav[F] with Red[F]` against `Trav[Tuple1] with Red[Tuple1]`).

trait Bi[F[_, _]] { def swapped[A, B](fa: F[A, B]): Any }
trait Trav[F[_]] { def one[A](fa: F[A]): A }
trait Red[F[_]] { def size[A](fa: F[A]): Int }

object Main {
  // 1
  def tupleCopy(): Unit = {
    val p2: (Int, String) = (1, "a")
    println(p2.copy(_2 = "b"))
    println(Tuple1(1).copy(_1 = 2))
    val p5 = (1, 2, 3, 4, 5)
    println(p5.copy(_3 = 30, _5 = 50))
    // `copy` re-infers the class's type parameters, as nsc's `copy[T1, T2]`
    // does: the result is not forced back to `(Int, String)`.
    val widened: (String, String) = p2.copy(_1 = "x")
    println(widened)
  }

  // 2 + 3
  def bi[F[_, _] <: Product](f: F[Any, Any] => Any): Bi[F] =
    new Bi[F] {
      def swapped[A, B](fa: F[A, B]): Any = f(fa.asInstanceOf[F[Any, Any]])
    }

  def biTuple3[A0]: Bi[({ type L[x, y] = (A0, x, y) })#L] =
    bi(fa => fa.copy(_2 = fa._3, _3 = fa._2))

  // 4
  def instance[F[_] <: Product](get: F[Any] => Any): Trav[F] with Red[F] =
    new Trav[F] with Red[F] {
      def one[A](fa: F[A]): A = get(fa.asInstanceOf[F[Any]]).asInstanceOf[A]
      def size[A](fa: F[A]): Int = fa.productArity
    }

  val forTuple1: Trav[Tuple1] with Red[Tuple1] = instance(fa => fa.copy(_1 = fa._1)._1)

  def main(args: Array[String]): Unit = {
    tupleCopy()
    println(biTuple3[String].swapped(("k", 1, 2)))
    println(forTuple1.one(Tuple1("z")))
    println(forTuple1.size(Tuple1("z")))
  }
}
