// The lambda parameter of `map` / `flatMap` / `foreach` / `withFilter` /
// `pipe` / `tap` is guessed from the receiver's *first* type argument when the
// signature has not settled it. That guess is for a parameter still written in
// the declaring class's own type parameter -- an inherited `IterableOps[A, CC,
// C]` member reached through a `List[Int]` -- and it must not overrule a
// parameter the receiver has already answered.
//
// `class Vd[+E, +A] { def map[B](f: A => B): Vd[E, B] }` read through a
// `fa: Vd[E, A]` states its parameter as the caller's own `A`, which is in
// scope and rigid. The guess replaced it with `args[0]`, the `E`:
//
//     found: (A) => B  required: (E) => Any
//
// cats writes every `Validated` / `Ior` / `NonEmptyMap` instance this way.
//
// Everything below prints, so a parameter type that merely type-checks cannot
// pass: the bodies use methods only the right type has.

sealed abstract class Vd[+E, +A] {
  def map[B](f: A => B): Vd[E, B] = this match {
    case Vd.Inv(e) => Vd.Inv(e)
    case Vd.Val(a) => Vd.Val(f(a))
  }
  def flatMap[EE >: E, B](f: A => Vd[EE, B]): Vd[EE, B] = this match {
    case Vd.Inv(e) => Vd.Inv(e)
    case Vd.Val(a) => f(a)
  }
  def show: String = this match {
    case Vd.Inv(e) => "Inv(" + e + ")"
    case Vd.Val(a) => "Val(" + a + ")"
  }
}

object Vd {
  final case class Inv[+E](e: E) extends Vd[E, Nothing]
  final case class Val[+A](a: A) extends Vd[Nothing, A]
}

trait Fun[F[_]] {
  def map[A, B](fa: F[A])(f: A => B): F[B]
}

object VdFun {
  // The instance cats writes: a type lambda over the *second* parameter, and
  // a body that forwards to the receiver's own `map`.
  def instance[E]: Fun[({ type L[x] = Vd[E, x] })#L] =
    new Fun[({ type L[x] = Vd[E, x] })#L] {
      override def map[A, B](fa: Vd[E, A])(f: A => B): Vd[E, B] = fa.map(f)
    }
}

object Main {
  // Written out, with no instance in the way: `fa.map(f)` on a two-parameter
  // covariant class.
  def direct[E, A, B](fa: Vd[E, A], f: A => B): Vd[E, B] = fa.map(f)

  def main(args: Array[String]): Unit = {
    val ok: Vd[Int, String] = Vd.Val("abc")
    val bad: Vd[Int, String] = Vd.Inv(7)

    // The lambda parameter must be the `String`, not the `Int`.
    println(direct[Int, String, Int](ok, s => s.length).show)
    println(direct[Int, String, Int](bad, s => s.length).show)

    val f = VdFun.instance[Int]
    println(f.map(ok)(s => s.toUpperCase).show)
    println(f.map(bad)(s => s.toUpperCase).show)

    // `flatMap` and `foreach` go through the same guess.
    println(ok.flatMap[Int, Int](s => Vd.Val(s.length * 2)).show)
    println(ok.map(s => s.reverse).show)
  }
}
