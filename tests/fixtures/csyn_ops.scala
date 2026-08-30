// cats' syntax classes are `Ops[F[_], A]`: the *first* type argument is a type
// constructor, not an element. `map` / `flatMap` / `foreach` used to take the
// receiver's first type argument for the lambda's parameter type, which is
// right for `List[A]` and wrong here -- `Ops[Box, Int].flatMap(n => …)` gave
// `n` the type `Box`, and the body was then checked against the wrong thing.
// It reproduces with no implicit conversion in sight: `new Ops[Box, Int](b)`.

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

final class Ops[F[_], A](val self: F[A]) {
  def flatMap[B](f: A => F[B])(implicit F: FlatMap[F]): F[B] = F.flatMap(self)(f)
  def map[B](f: A => B)(implicit F: FlatMap[F]): F[B] = F.map(self)(f)
  def foreach(f: A => Unit)(implicit F: FlatMap[F]): Unit = { F.map(self)(f); () }
}

object Main {
  def main(args: Array[String]): Unit = {
    val b = new Box(3)
    // `n` is an `Int`, so `n + 1` is arithmetic and not `any2stringadd`.
    println(new Ops[Box, Int](b).flatMap(n => new Box(n + 1)).a)
    println(new Ops[Box, Int](b).map(n => n * 2).a)
    new Ops[Box, Int](b).foreach(n => println(n + 100))
    // The same through an abstract `F[_]`.
    println(twice(new Box(20)).a)
  }

  def twice[F[_]](fa: F[Int])(implicit F: FlatMap[F]): F[Int] =
    new Ops[F, Int](fa).flatMap(n => F.map(fa)(m => m + n))
}
