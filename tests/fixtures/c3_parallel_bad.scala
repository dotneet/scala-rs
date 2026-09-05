// What the three fixes in `c3_parallel.scala` must still reject. scalac
// 2.13.16 rejects every one of these too.

trait FnK[F[_], G[_]] {
  def apply[A](fa: F[A]): G[A]
}
trait NEP[M[_]] {
  type F[_]
  def sequential: FnK[F, M]
}

object BadPar {
  // Solving the inserted `apply`'s `A` from the argument is not "believe the
  // expected type": the argument says `A = List[X]`, so the result is
  // `M[List[X]]` and does not conform to `M[X]`.
  def wrong[M[_], X](P: NEP[M])(fa: P.F[List[X]]): M[X] = P.sequential(fa)
}

object BadColl {
  // The rebuild puts the *receiver's* class back, never a narrower one: a
  // `Seq` really is a `Seq` here, and `tail` on it is not a `Vector`.
  def notAVector(xs: Seq[Int]): Vector[Int] = xs.tail
}

// The override half is in `c3_override_bad.scala`: scalac's override check
// runs in `refchecks`, after `typer`, so a file that already has a type error
// never reaches it and could not be dual-run here.
