// A higher-kinded call's result type parameter, read out of the *expected*
// type, and a case class's `copy` written inside the class itself.
//
// `F.flatMap(fa) { … }` on an abstract `F[_]` has an `Applied` result type
// (`F[B]`), which the expected-type walk did not know how to line up with
// `F[String]`: `B` was never solved and every cats-style call came back
// `F[Any]`.
//
// `copy(f = x)` inside `Cell` is the same call as `c.copy(f = x)` on `this`,
// and nsc's synthesized `copy[F]` re-infers the class's own type parameters --
// so a `Cell[Some[Int]]` may be rebuilt as a `Cell[Option[Int]]`.

trait FlatMap[F[_]] {
  def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]
  def map[A, B](fa: F[A])(f: A => B): F[B]
  def pure[A](a: A): F[A]
}

final class Box[A](val value: A)

object Box {
  implicit val boxFlatMap: FlatMap[Box] = new FlatMap[Box] {
    def flatMap[A, B](fa: Box[A])(f: A => Box[B]): Box[B] = f(fa.value)
    def map[A, B](fa: Box[A])(f: A => B): Box[B] = new Box(f(fa.value))
    def pure[A](a: A): Box[A] = new Box(a)
  }
}

object Chain {
  // `B` comes from the expected `F[String]`, not from the lambda.
  def twice[F[_]](fa: F[Int])(implicit F: FlatMap[F]): F[String] =
    F.flatMap(fa) { i => F.flatMap(fa) { j => F.pure((i + j).toString) } }

  // The one-level form, and the `map` counterpart.
  def once[F[_]](fa: F[Int])(implicit F: FlatMap[F]): F[String] =
    F.flatMap(fa)(i => F.pure(i.toString))

  def label[F[_]](fa: F[Int])(implicit F: FlatMap[F]): F[String] =
    F.map(fa)(i => "n=" + i)
}

final case class Cell[+F <: Option[Int]](name: String, f: F = None, g: Option[Int] = None) {
  def widen(x: Option[Int]): Cell[Option[Int]] = copy(f = x)
  def renamed(s: String): Cell[F] = copy(name = s)
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Chain.twice(new Box(21)).value)
    println(Chain.once(new Box(7)).value)
    println(Chain.label(new Box(5)).value)
    // The private runtime's `Option` has no `toString` of its own, so the
    // fields are printed rather than the case class.
    val c = Cell("a", Some(1))
    println(c.widen(Some(2)).f.getOrElse(0))
    println(c.renamed("z").name)
    println(c.copy().f.getOrElse(0))
    val emptied: Cell[Option[Int]] = c.copy(f = None)
    println(emptied.f.getOrElse(-1))
  }
}
