// Context bounds on higher-kinded type parameters. scalac 2.13.16 accepts
// `[F[_]: C]` on both defs and classes (it desugars to `(implicit ev: C[F])`);
// only *view* bounds on such a parameter are rejected.

trait Async[F[_]] {
  def pure[A](a: A): F[A]
  def show[A](fa: F[A]): String
}

class Holder[F[_]: Async](val n: Int) {
  def lift: F[Int] = implicitly[Async[F]].pure(n)
}

trait Mk {
  def makeDatabase[F[_]: Async](): F[Int] = implicitly[Async[F]].pure(1)
}

// A term named `F` must not hide the type parameter `F` in type position:
// Scala keeps terms and types in separate namespaces.
trait Shadowed[F[_]] {
  protected def asyncF: Async[F]
  def twice: String = {
    val F = asyncF
    val a: F[Int] = F.pure(2)
    F.show(a)
  }
}

object Main extends Mk {
  implicit val optAsync: Async[Option] = new Async[Option] {
    def pure[A](a: A): Option[A] = Some(a)
    def show[A](fa: Option[A]): String = fa.toString
  }

  object S extends Shadowed[Option] {
    protected def asyncF: Async[Option] = optAsync
  }

  def main(args: Array[String]): Unit = {
    println(optAsync.show(new Holder[Option](3).lift))
    println(optAsync.show(makeDatabase[Option]()))
    println(S.twice)
  }
}
