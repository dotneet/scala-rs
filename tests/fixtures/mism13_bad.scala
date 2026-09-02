// None of the six makes the typer more permissive. nsc 2.13.16 reports the
// same six errors here.

class Sh[T, U](val tag: String)

case class Box[T, U](value: T, shape: Sh[T, U]) {
  // The constructor's parameters are read at `[(T, T2), (U, U2)]`, once: a
  // `T` is still not a `(T, T2)`.
  def wrong[T2, U2](v2: T2, s2: Sh[T2, U2]): Box[(T, T2), (U, U2)] =
    new Box[(T, T2), (U, U2)](value, new Sh[(T, T2), (U, U2)]("x"))
}

class Bx[A](val a: A)
class By[A](val a: A)
object Nest {
  def h[B](f: Int => Bx[B]): Bx[B] = f(1)
  def take[F[_], A](x: A): A = x
}

abstract class Qy[+E, U, C[_]] {
  def label: String
  def map[F, G, T](f: E => F): Qy[G, T, C] = new Wrap[G, T, C]("m")
}
class Wrap[+E, U, C[_]](val label: String) extends Qy[E, U, C]
final class BJQ[+E1, +E2, U1, U2, C[_], +B1, +B2](l: String) extends Wrap[(E1, E2), (U1, U2), C](l)
class Uses[E, U, C[_], D[_]] {
  private[this] def join[E2, U2] = new BJQ[E, E2, U, U2, C, E, E2]("join")
  // `map`'s result is the receiver's `C`, never its `D`.
  def zipWith[E2, U2, F, G, T](f: E => F): Qy[G, T, D] =
    join[E2, U2].map[F, G, T](x => f(x._1))
}

final case class St[F[_]](available: Long, waiting: Bx[F[Long]])

class Rp[T](val s: String)
class Ext[P](val r: P) {
  def flatten[QO](implicit ev: P <:< Rp[Option[QO]]): Rp[Option[QO]] = r
}

object Main {
  // The relaxed expected type is a wildcard, not "anything": the body still
  // has to be a `Bx`.
  val wrongBody: Bx[Int] = Nest.h(i => new By(i))

  // The higher-kinded `apply` reads `F` off the expected type, and a `St[By]`
  // is not a `St[Bx]`.
  val wrongCtor: St[Bx] = Nest.take[Bx, St[By]](St[By](1L, null))

  // A view is still a view: `<:<` converts only what it is a witness for.
  val notAView: Rp[Option[Int]] = new Ext[String]("s").flatten[Int]

  // The branch join is still a join, and neither branch is the other.
  def branch(flag: Boolean): Bx[Int] = if (flag) new Bx("s") else new Bx(1)

  def main(args: Array[String]): Unit = println("unreachable")
}
