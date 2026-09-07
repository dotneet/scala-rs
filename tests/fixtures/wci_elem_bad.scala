// The parameter of `map`'s function is now read off the signature rather than
// guessed from `args[0]`. It therefore still has to be *checked*: a lambda
// written at the wrong type is a mismatch, and so is one written at the type
// the guess used to supply.
//
//   25  the receiver is a `Vd[Int, String]`, so `map`'s lambda takes a
//       `String`; an `Int` parameter is not it -- and `Int` is exactly what
//       the discarded guess would have handed it.
//   27  the same through a rigid `A`: `E` is not `A`.

sealed abstract class Vd[+E, +A] {
  def map[B](f: A => B): Vd[E, B] = this match {
    case Vd.Inv(e) => Vd.Inv(e)
    case Vd.Val(a) => Vd.Val(f(a))
  }
}

object Vd {
  final case class Inv[+E](e: E) extends Vd[E, Nothing]
  final case class Val[+A](a: A) extends Vd[Nothing, A]
}

object Bad {
  val v: Vd[Int, String] = Vd.Val("abc")
  val a: Vd[Int, Int] = v.map((n: Int) => n + 1)

  def through[E, A](fa: Vd[E, A]): Vd[E, Int] = fa.map((e: E) => e.toString.length)
}
