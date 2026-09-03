// The three fixes in `f2_cats.scala` must not turn into "resolve anything".
// Each case here is one real scalac rejects too, in the same file shape (the
// `fs2.Stream` mention included, so the same load order is in play).
package f2cats

import cats.effect.{Ref, Resource}

object F2CatsBad {
  def stream(s: fs2.Stream[cats.effect.IO, Int]): Int = 0

  // No `Async` / `Sync` / `Concurrent` anywhere in scope, so nothing can build
  // a `Ref.Make[F]`.
  def noMake[F[_]](n: Long): F[Ref[F, Long]] = Ref.of[F, Long](n)

  // `Resource` declares no such type, before or after the fix.
  def notAMember(x: Resource.NoSuchCase): Int = 0

  // `IO.fromFuture` wants an `IO[Future[A]]`. The by-name `apply` the fix
  // brings back from the pickle must not make an `IO[Int]` fit it.
  def notAFuture: cats.effect.IO[Int] = cats.effect.IO.fromFuture(cats.effect.IO(42))
}
