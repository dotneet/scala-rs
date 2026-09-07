// The half that says the two solutions really are solutions.
//
// Reading a variable out of an expected type that is only `_` is wrong; not
// reading one out of an expected type that *does* say something is equally
// wrong. These three are rejected by real scalac 2.13.16, and they are the
// cases the two rules here could have made compile by accident:
//
//   33  `flatTraverse` genuinely produces `P.F[T[B]]`, so a declared
//       `P.F[T[Int]]` must not be accepted just because `B` went unsolved.
//   50  the type lambda in `appForKle`'s result *does* fix `A`, so `A :=
//       Boolean` has to be looked for -- and there is no `Show[Boolean]`.
//   52  and once fixed, `A` has to agree: an explicit `Int` cannot serve a
//       declared `Boolean`.

trait FunK[F[_], G[_]] { def apply[A](fa: F[A]): G[A] }
trait Apl[G[_]]
trait FlatM[F[_]]

trait Trav[F[_]] {
  def flatTraverse[G[_], A, B](fa: F[A])(f: A => G[F[B]])(implicit G: Apl[G], F: FlatM[F]): G[F[B]]
}

trait Par[M[_]] {
  type F[_]
  def parallel: FunK[M, F]
  def applicative: Apl[F]
}

object Bad1 {
  def wrong[T[_], M[_], A, B](tv: Trav[T], ta: T[A], f: A => M[T[B]], fm: FlatM[T])(
    P: Par[M]
  ): P.F[T[Int]] =
    tv.flatTraverse(ta)(a => P.parallel(f(a)))(P.applicative, fm)
}

trait Apl2[G[_]]
trait Show[A]
final case class Kle[F[_], A, B](run: A => F[B])
final class Bx[A]

object Kle {
  def appForKle[F[_], A](implicit F: Apl2[F], A: Show[A]): Apl2[({ type L[x] = Kle[F, A, x] })#L] =
    new Apl2[({ type L[x] = Kle[F, A, x] })#L] {}
}

object Bad2 {
  implicit object bxApl extends Apl2[Bx]
  implicit object showInt extends Show[Int]

  val a: Apl2[({ type L[x] = Kle[Bx, Boolean, x] })#L] = Kle.appForKle

  val b: Apl2[({ type L[x] = Kle[Bx, Boolean, x] })#L] = Kle.appForKle[Bx, Int]
}
