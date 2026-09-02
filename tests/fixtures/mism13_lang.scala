// Five of the causes behind slick's remaining `type mismatch`es, all of them
// plain language rules, so this runs on the private runtime as well as
// against the real jar.
//
//  * A class instantiating *itself* at a type that mentions its own type
//    parameter (`new Box[(T, T2), (U, U2)]` inside `Box[T, U]`): the
//    constructor's parameters must be read at those arguments exactly once.
//  * A variable that only the lambda's *result* can fix
//    (`def h[B](f: Int => Bx[B])`).
//  * An inherited member is declared in its owner's type parameters, which
//    line up with the receiver's only when the receiver is that class
//    (`BJQ[E1, E2, U1, U2, C, B1, B2] <: Qy[+E, U, C[_]]`).
//  * Explicit type arguments settle a parameter, and that settled type is the
//    expected type of the argument (`take[F, St[F]](St(…))`, where `F` is
//    higher-kinded and no argument mentions it).
//  * The branches of an `if` are joined after a variable nothing pinned is
//    read at its bound (`if (c) Vec.empty else pairs`).

class Sh[T, U](val tag: String)

case class Box[T, U](value: T, shape: Sh[T, U]) {
  def zip[T2, U2](v2: T2, s2: Sh[T2, U2]): Box[(T, T2), (U, U2)] =
    new Box[(T, T2), (U, U2)]((value, v2), new Sh[(T, T2), (U, U2)](shape.tag + "+" + s2.tag))
}

class Bx[A](val a: A)
object Nest {
  def h[B](f: Int => Bx[B]): Bx[B] = f(1)
  def take[F[_], A](x: A): A = x
}

// The inherited member: `map`'s result names `Qy`'s own `C`, and the receiver
// is a `BJQ` whose fifth argument that is.
abstract class Qy[+E, U, C[_]] {
  def label: String
  def map[F, G, T](f: E => F): Qy[G, T, C] = new Wrap[G, T, C]("mapped(" + label + ")")
}
class Wrap[+E, U, C[_]](val label: String) extends Qy[E, U, C]
final class BJQ[+E1, +E2, U1, U2, C[_], +B1, +B2](l: String) extends Wrap[(E1, E2), (U1, U2), C](l)
class Uses[E, U, C[_]] {
  private[this] def join[E2, U2] = new BJQ[E, E2, U, U2, C, E, E2]("join")
  def zipWith[E2, U2, F, G, T](f: E => F): Qy[G, T, C] =
    join[E2, U2].map[F, G, T](x => f(x._1))
}

// The higher-kinded companion `apply`, decided by the expected type alone:
// `F` occurs in no argument the call writes out.
final case class St[F[_]](available: Long, waiting: Bx[F[Long]])

// A covariant `empty` whose parameter nothing constrains.
class Vec[+A](val head: A, val size: Int)
object Vec {
  def empty[A]: Vec[A] = new Vec[A](null.asInstanceOf[A], 0)
}

object Main {
  def branch(flag: Boolean, pair: (String, Int)): Vec[(String, Int)] = {
    val ch = if (flag) Vec.empty else new Vec(pair, 1)
    ch
  }

  def main(args: Array[String]): Unit = {
    val b = Box(1, new Sh[Int, String]("a"))
    val z = b.zip("s", new Sh[String, Long]("b"))
    println(z.value._1 + ":" + z.value._2)
    println(z.shape.tag)

    println(Nest.h(i => new Bx(i + 1)).a)

    val u = new Uses[Int, String, Bx]
    println(u.zipWith[Long, Long, String, String, String](x => x.toString).label)

    val s = Nest.take[Bx, St[Bx]](St(3L, null))
    println(s.available)

    println(branch(true, ("k", 1)).size)
    println(branch(false, ("k", 1)).head._1)
  }
}
