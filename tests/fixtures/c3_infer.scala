// agent/cats3: two places where a callee's type was read too raw.
//
// 1. `>>[B](fb: => F[B])(implicit ev: Bind[F])`: the expected type says
//    `B = Unit`, so the by-name formal is the argument's prototype. Without
//    it `good.fold(boom, _ => new Box(()))` was typed against nothing and came
//    back as the lub of `Box[Nothing]` and `Box[Unit]`.
// 2. `tag` is *overloaded*, so the picked alternative's type was re-read from
//    the declaration -- in `TagOps`'s own `F`, not the receiver's -- and its
//    `implicit t: TC[F, _]` could match nothing.

final class Box[A](val value: A) {
  override def toString: String = "Box(" + value + ")"
}

trait Bind[F[_]] {
  def bind[A, B](fa: F[A], fb: F[B]): F[B]
}

trait TC[F[_], E] {
  def name: String
}

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
  implicit val boxTC: TC[Box, Throwable] = new TC[Box, Throwable] {
    def name: String = "box"
  }

  def boom[A](m: String): Box[A] = throw new RuntimeException(m)

  def main(args: Array[String]): Unit = {
    val good: Res[Int] = Good(3)
    val bad: Res[Int] = Bad("nope")

    val out: Box[Unit] = new ThenOps[Box, Int](new Box(1)) >> good.fold(boom, _ => new Box(()))
    println(out)

    // The prototype only *guides* inference; the branch that throws still
    // throws.
    val threw =
      try {
        val ignored: Box[Unit] =
          new ThenOps[Box, Int](new Box(1)) >> bad.fold(boom, _ => new Box(()))
        ignored.toString
      } catch {
        case e: RuntimeException => "caught " + e.getMessage
      }
    println(threw)

    println(new TagOps[Box, Int](new Box(1)).tag(7))
    println(new TagOps[Box, String](new Box("s")).tag("!"))
  }
}
