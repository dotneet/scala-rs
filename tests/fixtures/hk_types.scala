class Id[A](val value: A)
class Box[F[_], A](val fa: F[A])
trait Functor[F[_]] {
  def map[A, B](fa: F[A])(f: A => B): F[B]
}
object IdFunctor extends Functor[Id] {
  def map[A, B](fa: Id[A])(f: A => B): Id[B] = new Id(f(fa.value))
}
object Main {
  def main(args: Array[String]): Unit = {
    val b = new Box[Id, Int](new Id(41))
    println(b.fa.value)
    println(IdFunctor.map[Int, Int](new Id(1))((x: Int) => x + 1).value)
  }
}
