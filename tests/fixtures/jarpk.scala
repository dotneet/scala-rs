// Higher-kinded type classes: the shape a `-cp` jar's `ScalaSignature` has to
// survive. A JVM generic signature writes both the `F` of `Monadic[F[_]]` and
// the `F[A]` of `pure[A](a: A): F[A]` as a bare `TF;`, so a class read that way
// is a kind error at every use site and `F.pure(v)` is `found: F required:
// F[Int]`. Read from the pickle instead, it is this program.
//
// `crates/cli/tests/jarpickle.rs` compiles the same shapes into a jar and reads
// them back through `-cp`; this fixture pins the source-level meaning the jar
// round trip has to preserve.
trait Functor[F[_]] {
  def fmap[A, B](fa: F[A])(f: A => B): F[B]
}

trait Monadic[F[_]] extends Functor[F] {
  def pure[A](a: A): F[A]
  def bind[A, B](fa: F[A])(f: A => F[B]): F[B]
}

class Ident[A](val value: A) {
  override def toString: String = "Ident(" + value.toString + ")"
}

object Instances {
  implicit val optionMonadic: Monadic[Option] = new Monadic[Option] {
    def pure[A](a: A): Option[A] = Some(a)
    def bind[A, B](fa: Option[A])(f: A => Option[B]): Option[B] = fa.flatMap(f)
    def fmap[A, B](fa: Option[A])(f: A => B): Option[B] = fa.map(f)
  }

  implicit val listMonadic: Monadic[List] = new Monadic[List] {
    def pure[A](a: A): List[A] = List(a)
    def bind[A, B](fa: List[A])(f: A => List[B]): List[B] = fa.flatMap(f)
    def fmap[A, B](fa: List[A])(f: A => B): List[B] = fa.map(f)
  }

  implicit val identMonadic: Monadic[Ident] = new Monadic[Ident] {
    def pure[A](a: A): Ident[A] = new Ident[A](a)
    def bind[A, B](fa: Ident[A])(f: A => Ident[B]): Ident[B] = f(fa.value)
    def fmap[A, B](fa: Ident[A])(f: A => B): Ident[B] = new Ident[B](f(fa.value))
  }
}

object Main {
  def liftInt[F[_]](n: Int)(implicit F: Monadic[F]): F[Int] = F.pure(n)

  def twice[F[_], A](fa: F[A])(f: A => A)(implicit F: Monadic[F]): F[A] =
    F.fmap(F.fmap(fa)(f))(f)

  def chain[F[_]](fa: F[Int])(implicit F: Monadic[F]): F[Int] =
    F.bind(fa)((n: Int) => F.pure(n * 10))

  def describe[F[_]](fa: F[Int])(implicit F: Functor[F]): F[String] =
    F.fmap(fa)((n: Int) => "n=" + n.toString)

  def main(args: Array[String]): Unit = {
    import Instances._
    val inc = (x: Int) => x + 1
    println(liftInt[Option](7))
    println(liftInt[List](8))
    println(liftInt[Ident](9))
    println(twice(Option(1))(inc))
    println(twice(List(1, 2))(inc))
    println(twice(new Ident[Int](5))(inc))
    println(chain(Option(2)))
    println(chain(List(3, 4)))
    println(describe(Option(3)))
    println(describe(List(4, 5)))
  }
}
