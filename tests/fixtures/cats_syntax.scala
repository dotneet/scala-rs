// cats' syntax layer is an implicit conversion that is *itself* higher-kinded
// and takes an implicit clause of its own:
//   implicit def toFlatMapOps[F[_], A](fa: F[A])(implicit F: FlatMap[F])
// Two things had to work for `fa.flatMap(…)` on an `F[A]` to compile and run:
// solving `F` from the receiver's type *constructor* rather than from one of
// its arguments (it used to fall through to `AnyRef`), and applying the
// conversion's own implicit clause (it used to be dropped, so codegen emitted
// a call one argument short of the descriptor and the verifier rejected it).

trait Functor[F[_]] {
  def map[A, B](fa: F[A])(f: A => B): F[B]
}

trait FlatMap[F[_]] extends Functor[F] {
  def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]
}

final class Box[A](val a: A)

object Box {
  implicit def flatMapForBox: FlatMap[Box] = new FlatMap[Box] {
    def map[A, B](fa: Box[A])(f: A => B): Box[B] = new Box(f(fa.a))
    def flatMap[A, B](fa: Box[A])(f: A => Box[B]): Box[B] = f(fa.a)
  }
}

final class FlatMapOps[F[_], A](val fa: F[A]) {
  def flatMap[B](f: A => F[B])(implicit F: FlatMap[F]): F[B] = F.flatMap(fa)(f)
  /// No type parameter of its own: `A` and `F` come from the conversion alone.
  def unwrap: F[A] = fa
}

object syntax {
  implicit def toFlatMapOps[F[_], A](fa: F[A])(implicit F: FlatMap[F]): FlatMapOps[F, A] =
    new FlatMapOps(fa)
}

import syntax._

object Main {
  // abstract `F[_]`: the conversion has to solve `F` to the *type parameter*
  def twice[F[_]](fa: F[Int])(implicit F: FlatMap[F]): F[Int] = fa.flatMap(_ => fa)

  // concrete receiver: `F` is solved to the class `Box`, `A` to `Int`
  def same(b: Box[Int]): Box[Int] = b.unwrap

  def main(args: Array[String]): Unit = {
    println(twice(new Box(3)).a)
    println(same(new Box(41)).a)
    // the companion's implicit is what `FlatMap[Box]` resolves to
    println(implicitly[FlatMap[Box]].flatMap(new Box(7))(n => new Box(n * 2)).a)
  }
}
