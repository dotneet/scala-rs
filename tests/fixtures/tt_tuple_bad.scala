// The tuple / type-lambda shapes nsc rejects, so that the widening in
// `tt_tuple.scala` is pinned to stop where nsc stops. scalac 2.13.16 rejects
// every one of these four.

trait Bi[F[_, _]] { def swapped[A, B](fa: F[A, B]): Any }

object Main {
  val p: (Int, String) = (1, "a")

  // A tuple has no `_3`, so `copy` has no such named argument.
  val noSuchField = p.copy(_3 = 1)

  // `copy` re-infers the type parameters, so the result is `(String, String)`
  // and does not conform to the ascription.
  val wrongResult: (Int, String) = p.copy(_1 = "x")

  def bi[F[_, _] <: Product](f: F[Any, Any] => Any): Bi[F] =
    new Bi[F] {
      def swapped[A, B](fa: F[A, B]): Any = f(fa.asInstanceOf[F[Any, Any]])
    }

  // Reducing the type lambda offers exactly its body's members, no more: a
  // `Tuple3` has no `_4`.
  def noSuchMember[A0]: Bi[({ type L[x, y] = (A0, x, y) })#L] = bi(fa => fa._4)

  // `Int` is not a `Product`, so the type lambda does not meet the bound.
  val notAProduct: Bi[({ type L[x, y] = Int })#L] = bi(fa => fa)
}
