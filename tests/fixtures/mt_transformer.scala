// The shapes cats' monad transformers are built out of: `EitherT`, `IorT` and
// `OptionT` all wrap an `F[…]` and rebuild themselves by handing a
// pattern-matching anonymous function to `F.flatMap`. Four things had to work
// before any of that typed, and none of them needs cats to show:
//
//  1. A lambda whose body is a `match` (or an `if`) has to *decide* the
//     method type parameter it is checked against. `F.flatMap(value) { case … }`
//     is checked at `X => F[_]`, and the answer used to be `F[_]` again --
//     `EitherT(…)` then reported `no matching overload for
//     (F[Either[A, B]])EitherT[F, A, B] with arguments (F[_])`.
//  2. The branches need not agree: `orElse` returns an `F[Either[C, BB]]` from
//     one and an `F[Right[C, BB]]` from the other, and the argument the caller
//     meant is the join of the two *arguments*, not of the two applications
//     (`F` is an abstract constructor, so joining the applications cannot get
//     past its invariance).
//  3. A type constructor parameter that only the *compound* result type
//     mentions -- `def instance[F[_]](…): Trav[F] with Red[F]` -- has to come
//     off the expected type.
//  4. A case class whose parent drops or reorders the sealed parent's type
//     parameters (`Ior.Right[+B] extends Ior[Nothing, B]`) must bind its
//     pattern variable at the scrutinee's matching argument, not by position.
//
// `--scala-library` only: `Either`, `Tuple1` and `Product with Serializable`
// come from the real library.
object MT {

  sealed abstract class Ior[+A, +B] extends Product with Serializable
  object Ior {
    final case class Left[+A](a: A) extends Ior[A, Nothing]
    // Spelled infix, the way cats spells it.
    final case class Right[+B](b: B) extends (Nothing Ior B)
    final case class Both[+A, +B](a: A, b: B) extends Ior[A, B]
  }

  trait Monad[F[_]] {
    def pure[A](a: A): F[A]
    def map[A, B](fa: F[A])(f: A => B): F[B]
    def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]
  }

  implicit val optionMonad: Monad[Option] = new Monad[Option] {
    def pure[A](a: A): Option[A] = Some(a)
    def map[A, B](fa: Option[A])(f: A => B): Option[B] = fa.map(f)
    def flatMap[A, B](fa: Option[A])(f: A => Option[B]): Option[B] = fa.flatMap(f)
  }

  def leftCast[A, B, C](r: Right[A, B]): Right[C, B] = r.asInstanceOf[Right[C, B]]

  final case class EitherT[F[_], A, B](value: F[Either[A, B]]) {

    // (1) Nothing but the case-lambda's body says what `flatMap`'s second
    // type parameter is.
    def biflatMap[C, D](fa: A => EitherT[F, C, D], fb: B => EitherT[F, C, D])(implicit
      F: Monad[F]
    ): EitherT[F, C, D] =
      EitherT(F.flatMap(value) {
        case Left(a)  => fa(a).value
        case Right(b) => fb(b).value
      })

    // (2) The two branches have different types.
    def orElse[C, BB >: B](default: F[Either[C, BB]])(implicit F: Monad[F]): EitherT[F, C, BB] =
      EitherT(F.flatMap(value) {
        case Left(_)      => default
        case r @ Right(_) => F.pure(leftCast(r))
      })

    // (2), in its `if` form.
    def orIf[C, BB >: B](cond: Boolean, default: F[Either[C, BB]])(implicit
      F: Monad[F]
    ): EitherT[F, C, BB] =
      EitherT(if (cond) default else F.map(value)(e => leftCast(e.asInstanceOf[Right[A, BB]])))
  }

  final case class IorT[F[_], A, B](value: F[Ior[A, B]]) {

    // (4) `b` is the scrutinee's second argument; `a` is its first.
    def bimap[C, D](fa: A => C, fb: B => D)(implicit F: Monad[F]): IorT[F, C, D] =
      IorT(F.map(value) {
        case Ior.Left(a)    => Ior.Left(fa(a))
        case Ior.Right(b)   => Ior.Right(fb(b))
        case Ior.Both(a, b) => Ior.Both(fa(a), fb(b))
      })

    // (1) again, over three cases, with `F.pure` in one of them.
    def leftFlatMap[C](f: A => IorT[F, C, B])(implicit F: Monad[F]): IorT[F, C, B] =
      IorT(F.flatMap(value) {
        case Ior.Left(a)      => f(a).value
        case r @ Ior.Right(_) => F.pure(r.asInstanceOf[Ior[C, B]])
        case Ior.Both(a, _)   => f(a).value
      })
  }

  // (3) `F` appears nowhere but in the compound result type.
  trait Trav[F[_]] { def one[A](fa: F[A]): A }
  trait Red[F[_]] { def size[A](fa: F[A]): Int }

  private def instance[F[_]](get: F[Any] => Any): Trav[F] with Red[F] =
    new Trav[F] with Red[F] {
      def one[A](fa: F[A]): A = get(fa.asInstanceOf[F[Any]]).asInstanceOf[A]
      def size[A](fa: F[A]): Int = 1
    }

  implicit val tuple1Instance: Trav[Tuple1] with Red[Tuple1] = instance(fa => fa._1)
}

object Main {
  import MT._

  def main(args: Array[String]): Unit = {
    val r: EitherT[Option, String, Int] = EitherT(Some(Right(7): Either[String, Int]))
    val l: EitherT[Option, String, Int] = EitherT(Some(Left("boom"): Either[String, Int]))

    val bi = (e: EitherT[Option, String, Int]) =>
      e.biflatMap[Int, String](
        a => EitherT(Some(Left(a.length): Either[Int, String])),
        b => EitherT(Some(Right(b.toString): Either[Int, String]))
      )
    println(bi(r).value)
    println(bi(l).value)

    val fallback: Option[Either[Boolean, Int]] = Some(Left(true))
    println(r.orElse(fallback).value)
    println(l.orElse(fallback).value)
    println(r.orIf(false, fallback).value)
    println(l.orIf(true, fallback).value)

    val il: IorT[Option, String, Int] = IorT(Some(Ior.Left("a"): Ior[String, Int]))
    val ir: IorT[Option, String, Int] = IorT(Some(Ior.Right(2): Ior[String, Int]))
    val ib: IorT[Option, String, Int] = IorT(Some(Ior.Both("a", 2): Ior[String, Int]))
    println(il.bimap(_.length, _ + 1).value)
    println(ir.bimap(_.length, _ + 1).value)
    println(ib.bimap(_.length, _ + 1).value)

    val lf = (i: IorT[Option, String, Int]) =>
      i.leftFlatMap[Int](a => IorT(Some(Ior.Left(a.length): Ior[Int, Int])))
    println(lf(il).value)
    println(lf(ir).value)
    println(lf(ib).value)

    println(tuple1Instance.one(Tuple1("only")))
    println(tuple1Instance.size(Tuple1("only")))
  }
}
