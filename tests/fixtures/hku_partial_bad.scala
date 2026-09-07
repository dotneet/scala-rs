// The negative half of `hku_partial.scala`: partial unification abstracts the
// rightmost arguments and nothing else. Each of these would type-check under
// some *other* abstraction, and real scalac 2.13.16 rejects all of them.
object Main {
  class Inv[A]
  def pick[F[_], A](fa: F[A]): F[A] = fa
  def both[F[_], A](fa: F[A], a: Inv[A]): Int = 0
  def two[F[_, _], A, B](fab: F[A, B]): Int = 0
  class Foo[A, M[_]]
  def hk[G[_], A](ga: G[A]): Int = 0

  val e: Either[String, Int] = Right(1)

  // `[x]Either[x, Int]` would make `A := String` fit; nsc captures `String`
  // and abstracts `Int`, so `Inv[String]` is not an `Inv[Int]`.
  val ambiguous: Int = both(e, new Inv[String])

  // The expected type asks for the other abstraction of the same class.
  val wrongWay: Either[Int, String] = pick(e)

  // A two-parameter variable against a one-parameter type: nothing to capture.
  val arity: Int = two(Option(1))

  // `G[A]` against `Foo[Int, List]`: the rightmost parameter is a constructor,
  // not a proper type, so the kinds do not unify.
  val kinds: Int = hk(new Foo[Int, List])
}
