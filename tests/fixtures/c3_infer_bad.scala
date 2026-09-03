// agent/cats3: the two fixes must not turn into "accept anything".
//
// * A prototype guides inference; it does not license a value that was
//   *already* inferred without one. `x` below is the lub, and passing it is a
//   mismatch -- scalac 2.13.16 rejects the same line.
// * An overloaded member's implicit clause is now read at the receiver's own
//   arguments, so a witness for the wrong type constructor is still no witness.

final class Box[A](val value: A)

trait Bind[F[_]] {
  def bind[A, B](fa: F[A], fb: F[B]): F[B]
}

trait TC[F[_], E] {
  def name: String
}

final class Other[A](val a: A)

final class ThenOps[F[_], A](val fa: F[A]) {
  def >>[B](fb: => F[B])(implicit ev: Bind[F]): F[B] = ev.bind(fa, fb)
}

final class TagOps[F[_], A](val fa: F[A]) {
  def tag(n: Int)(implicit t: TC[F, _]): String = t.name + n
  def tag(s: String)(implicit t: TC[F, _]): String = t.name + s
}

sealed abstract class Res[+A] {
  def fold[C](bad: String => C, good: A => C): C = this match {
    case Bad(m)  => bad(m)
    case Good(a) => good(a)
  }
}
final case class Bad(msg: String) extends Res[Nothing]
final case class Good[A](a: A) extends Res[A]

object Main {
  implicit val boxBind: Bind[Box] = new Bind[Box] {
    def bind[A, B](fa: Box[A], fb: Box[B]): Box[B] = fb
  }
  // A witness for `Other`, not for `Box`.
  implicit val otherTC: TC[Other, Throwable] = new TC[Other, Throwable] {
    def name: String = "other"
  }

  def boom[A](m: String): Box[A] = throw new RuntimeException(m)

  def main(args: Array[String]): Unit = {
    val good: Res[Int] = Good(3)
    val x = good.fold(boom, _ => new Box(()))
    val bad1: Box[Unit] = new ThenOps[Box, Int](new Box(1)) >> x
    println(bad1)

    val bad2: String = new TagOps[Box, Int](new Box(1)).tag(7)
    println(bad2)
  }
}
