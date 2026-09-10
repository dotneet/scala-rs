trait Base[F[_]]
trait Sub[F[_]] extends Base[F]
class C {
  protected def choose[F[_], A](xs: F[A])(s: Sub[F]): String = "old"
  def choose[F[_], A](xs: F[A])(implicit b: Base[F]): String = "new"
  def call[F[_], A](xs: F[A], s: Sub[F]): String = {
    implicit def b: Base[F] = s
    choose(xs)
  }
}
object Main {
  def main(args: Array[String]): Unit =
    println(new C().call(List(1), new Sub[List] {}))
}

