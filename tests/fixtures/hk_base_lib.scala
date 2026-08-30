// The cats-shaped case: a `C[F[_]]` instance given as an object name. The
// argument's type is `OC.type`, and `C[Option]` is its base type.
object Main {
  trait C[F[_]] { def pure[A](a: A): F[A] }
  object OC extends C[Option] { def pure[A](a: A) = Some(a) }
  class LC extends C[List] { def pure[A](a: A) = List(a) }
  def use[F[_]](c: C[F]): F[Int] = c.pure(1)

  trait D[A]
  object OD extends D[Int]
  def firstOrder[A](d: D[A], a: A): A = a

  def main(args: Array[String]): Unit = {
    println(use(OC))
    println(use(new LC))
    println(use[Option](OC))
    println(firstOrder(OD, 42))
  }
}
