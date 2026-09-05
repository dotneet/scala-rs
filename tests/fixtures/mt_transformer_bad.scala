// The negative half of `mt_transformer.scala`. Deciding an undetermined type
// parameter from an `if`/`match` body, and lining an extractor up with the
// scrutinee, must not turn into "accept whatever the branches say".
//
// `--scala-library` only, like the positive fixture.
object MTBad {

  sealed abstract class Ior[+A, +B] extends Product with Serializable
  object Ior {
    final case class Left[+A](a: A) extends Ior[A, Nothing]
    final case class Right[+B](b: B) extends Ior[Nothing, B]
  }

  trait Monad[F[_]] {
    def pure[A](a: A): F[A]
    def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]
  }

  final case class EitherT[F[_], A, B](value: F[Either[A, B]])

  // A branch that is not an `F[…]` at all. The stand-in is filled from the
  // branches only when every one of them applies the same constructor; here
  // nothing decides the parameter, and the call has to be rejected.
  def notAnF[F[_], A, B](value: F[Either[A, B]])(implicit F: Monad[F]): EitherT[F, A, B] =
    EitherT(F.flatMap(value) {
      case Left(_)  => "not an F at all"
      case Right(_) => value
    })

  // `Ior.Right[+B] extends Ior[Nothing, B]`, so `b` is a `B`. Returning it
  // where the scrutinee's `A` is required is still a mismatch -- aligning the
  // extractor with the scrutinee is not licence to pick whichever side fits.
  def wrongSide[A, B](i: Ior[A, B]): A = i match {
    case Ior.Right(b) => b
    case _            => sys.error("no")
  }

  // A one-parameter `Left` in someone else's namespace is not
  // `scala.util.Left`: it does not conform to an `Either`.
  def notScalaLeft(): Either[String, Int] = Ior.Left("x")
}
