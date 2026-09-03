// The cats-effect corner slick's `basic/` and `dbio/` live in. Every case here
// failed on `main`; two of them only when the *same* compilation also mentions
// `fs2.Stream`, which is what made them look like whole-program-only failures.
//
// Compiled against real cats-effect / fs2 jars by `crates/cli/tests/final2.rs`,
// and accepted by real scalac 2.13.16.
package f2cats

import scala.collection.immutable.TreeMap
import scala.concurrent.Future

import cats.effect.{Async, Ref, Resource}

object F2Cats {
  final case class State[F[_]](available: Long, waiting: TreeMap[Long, Int])

  // 1. `Ref.of` takes an implicit `Ref.Make[F]`, and every witness for it is
  //    inherited into the companion of the *nested* trait `Ref.Make`. That
  //    companion is `cats/effect/kernel/Ref$Make$`, and it used to be installed
  //    in the *package*, where `Make`'s implicit scope could not see it.
  //    slick: `basic/ConcurrencyControl.scala:202`.
  def arbiter[F[_]](max: Long)(implicit F: Async[F]): F[Ref[F, State[F]]] =
    Ref.of[F, State[F]](State(max, TreeMap.empty))

  // 2. Reading `fs2/Stream.class` enters `cats/effect/kernel/Resource$ExitCase`
  //    under the *trait* `Resource`, because a nested class file's name does
  //    not say whether the object or the class declares it. `Resource.ExitCase`
  //    then looked the name up on `Resource$` and found nothing.
  //    slick: `basic/BasicBackend.scala:421`.
  def stream(s: fs2.Stream[cats.effect.IO, Int]): Int = 0

  def errorOf(exitCase: Resource.ExitCase): Option[Throwable] = exitCase match {
    case Resource.ExitCase.Errored(e) => Some(e)
    case Resource.ExitCase.Canceled   => Some(new java.util.concurrent.CancellationException("x"))
    case _                            => None
  }

  def succeeded(exitCase: Resource.ExitCase): Boolean = exitCase == Resource.ExitCase.Succeeded

  // 3. `cats.effect.IO(fa)` is `IO.apply(thunk: => A)`. A class file cannot
  //    write a by-name parameter, so the reader's copy is `apply(Function0[A])`
  //    and a `Future[R]` fits nothing. slick: `dbio/DBIOAction.scala:237`.
  def from[R](fa: Future[R]): cats.effect.IO[R] =
    cats.effect.IO.fromFuture(cats.effect.IO(fa))
}
