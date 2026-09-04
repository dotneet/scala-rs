// Type lambdas that real scalac 2.13.16 rejects. Comparing lambdas by their
// bodies must not turn into "every lambda fits every other one".
//
// scalac reports, in order: two type mismatches, a kind error, and a missing
// member of the refinement.
object Main {
  trait Functor[F[_]] { def map[A, B](fa: F[A])(f: A => B): F[B] }

  final case class Box[A](value: A)
  final case class Cup[A](value: A)
  final case class Pair[E, A](e: E, a: A)

  // A different constructor is a different lambda.
  val wrong: Functor[({ type L[a] = Box[a] })#L] = new Functor[Cup] {
    def map[A, B](fa: Cup[A])(f: A => B): Cup[B] = Cup(f(fa.value))
  }

  // Same shape, different captured argument.
  val wrong2: Functor[({ type L[a] = Pair[String, a] })#L] =
    new Functor[({ type L[a] = Pair[Int, a] })#L] {
      def map[A, B](fa: Pair[Int, A])(f: A => B): Pair[Int, B] = Pair(fa.e, f(fa.a))
    }

  // A binary lambda is not a `F[_]`.
  val arity: Functor[({ type L[a, b] = Pair[a, b] })#L] = null

  // No such member in the refinement.
  val nope: Functor[({ type L[a] = Box[a] })#M] = null

  def main(args: Array[String]): Unit = println("unreachable")
}
