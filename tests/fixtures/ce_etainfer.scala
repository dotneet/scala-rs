// Eta-expanding a polymorphic method in an argument position.
//
// `F.map(fa)(Ior.left)` hands `Ior.left` the expected type `A => _`: `map`'s
// own `B` is still undetermined, so the result of the function parameter is
// opened to a wildcard. That pins `Ior.left`'s `A` and says nothing about its
// `B`. The `B` left over is a *variable* the enclosing call has to solve, not
// a type -- cats' `IorT`, `EitherT` and `OptionT` are built out of exactly
// this shape, and it used to report `type mismatch; found: IorT[F, A, B]
// required: IorT[F, A, B]` because the leftover `B` symbol printed under the
// same name as the class's own.
//
// Everything here runs, so an instantiation that merely makes the file
// compile (`Nothing`, `Any`) cannot pass: the values are printed.

sealed trait Ior[+A, +B]
object Ior {
  final case class Left[+A](a: A) extends Ior[A, Nothing]
  final case class Right[+B](b: B) extends Ior[Nothing, B]
  final case class Both[+A, +B](a: A, b: B) extends Ior[A, B]
  def left[A, B](a: A): Ior[A, B] = Left(a)
  def right[A, B](b: B): Ior[A, B] = Right(b)
  def both[A, B](a: A, b: B): Ior[A, B] = Both(a, b)
}

trait Functor[F[_]] { def map[A, B](fa: F[A])(f: A => B): F[B] }

object Instances {
  implicit val optionFunctor: Functor[Option] = new Functor[Option] {
    def map[A, B](fa: Option[A])(f: A => B): Option[B] = fa.map(f)
  }
  implicit val listFunctor: Functor[List] = new Functor[List] {
    def map[A, B](fa: List[A])(f: A => B): List[B] = fa.map(f)
  }
}

final case class IorT[F[_], A, B](value: F[Ior[A, B]])

object IorT {
  // The cats shape: a value class carrying the one type argument that is
  // written, with the rest inferred at `apply`.
  final class LeftPartiallyApplied[B](private val dummy: Boolean = true) extends AnyVal {
    def apply[F[_], A](fa: F[A])(implicit F: Functor[F]): IorT[F, A, B] =
      IorT(F.map(fa)(Ior.left))
  }
  def left[B]: LeftPartiallyApplied[B] = new LeftPartiallyApplied[B]

  final class RightPartiallyApplied[A](private val dummy: Boolean = true) extends AnyVal {
    def apply[F[_], B](fb: F[B])(implicit F: Functor[F]): IorT[F, A, B] =
      IorT(F.map(fb)(Ior.right))
  }
  def right[A]: RightPartiallyApplied[A] = new RightPartiallyApplied[A]

  // The receiver of the inserted `apply` is itself polymorphic: `right`'s own
  // `A` is fixed by nothing until this result meets the declared type. cats
  // writes `liftF` and `liftK` exactly like this.
  def liftF[F[_], A, B](fb: F[B])(implicit F: Functor[F]): IorT[F, A, B] = right(fb)

  def liftBoxed[F[_], A, B](fb: F[B])(implicit F: Functor[F]): Box[IorT[F, A, B]] =
    Box(right(fb))
}

final case class Box[T](v: T)

object Direct {
  // No wrapper at all: the expected type is written on the method, and the
  // eta-expansion has both parameters to read.
  def widenLeft[F[_], A, B](fa: F[A])(implicit F: Functor[F]): F[Ior[A, B]] =
    F.map(fa)(Ior.left)

  // One generic wrapper between the eta-expansion and the expected type.
  def boxLeft[F[_], A, B](fa: F[A])(implicit F: Functor[F]): Box[F[Ior[A, B]]] =
    Box(F.map(fa)(Ior.left))

  // Two of them, so the variable has to travel out through both.
  def boxBoxRight[F[_], A, B](fb: F[B])(implicit F: Functor[F]): Box[Box[F[Ior[A, B]]]] =
    Box(Box(F.map(fb)(Ior.right)))

  // A leftover variable with nothing to solve it is its lower bound, as it is
  // for any other undetermined variable.
  def anyLeft[F[_], A](fa: F[A])(implicit F: Functor[F]): F[Ior[A, Nothing]] =
    F.map(fa)(Ior.left)
}

object Main {
  import Instances._

  def show(o: Any): String = o.toString

  def main(args: Array[String]): Unit = {
    val l: IorT[Option, String, Int] = IorT.left[Int](Option("boom"))
    println(show(l.value))

    val r: IorT[Option, String, Int] = IorT.right[String](Option(7))
    println(show(r.value))

    val ls: IorT[List, String, Int] = IorT.left[Int](List("a", "b"))
    println(show(ls.value))

    // The static type is what the wrong instantiation got wrong; pin it by
    // consuming the value at the declared element type.
    val back: Int = l.value match {
      case Some(Ior.Left(s)) => s.length
      case _                 => -1
    }
    println(show(back))

    val w: Option[Ior[String, Int]] = Direct.widenLeft[Option, String, Int](Option("x"))
    println(show(w))

    val b: Box[List[Ior[Int, String]]] = Direct.boxLeft[List, Int, String](List(1, 2))
    println(show(b))

    val bb: Box[Box[Option[Ior[Long, String]]]] =
      Direct.boxBoxRight[Option, Long, String](Option("deep"))
    println(show(bb))

    val n: Option[Ior[Char, Nothing]] = Direct.anyLeft[Option, Char](Option('c'))
    println(show(n))

    val lifted: IorT[Option, String, Int] = IorT.liftF[Option, String, Int](Option(11))
    println(show(lifted.value))

    val liftedBox: Box[IorT[List, Long, String]] =
      IorT.liftBoxed[List, Long, String](List("u", "v"))
    println(show(liftedBox))

    // A concrete expected function type still solves both parameters up
    // front, with no undetermined variable in play at all.
    val f: String => Ior[String, Int] = Ior.left
    println(show(f("plain")))
  }
}
