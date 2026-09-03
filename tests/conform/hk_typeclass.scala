// Higher-kinded type classes in cats' shape: Functor/Monad with instances for
// Option and List, a context bound, and syntax through an implicit class that
// carries an implicit parameter list of its own.
object Main {
  trait Functor[F[_]] { def map[A, B](fa: F[A])(f: A => B): F[B] }
  trait Monad[F[_]] extends Functor[F] {
    def pure[A](a: A): F[A]
    def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]
    def map[A, B](fa: F[A])(f: A => B): F[B] = flatMap(fa)(a => pure(f(a)))
    def map2[A, B, C](fa: F[A], fb: F[B])(f: (A, B) => C): F[C] =
      flatMap(fa)(a => map(fb)(b => f(a, b)))
  }
  object Monad {
    implicit val optM: Monad[Option] = new Monad[Option] {
      def pure[A](a: A) = Some(a)
      def flatMap[A, B](fa: Option[A])(f: A => Option[B]) = fa.flatMap(f)
    }
    implicit val listM: Monad[List] = new Monad[List] {
      def pure[A](a: A) = List(a)
      def flatMap[A, B](fa: List[A])(f: A => List[B]) = fa.flatMap(f)
    }
    def apply[F[_]](implicit m: Monad[F]): Monad[F] = m
  }

  implicit class MonadOps[F[_], A](fa: F[A])(implicit m: Monad[F]) {
    def fmap[B](f: A => B): F[B] = m.map(fa)(f)
    def bind[B](f: A => F[B]): F[B] = m.flatMap(fa)(f)
  }

  def traverseOpt[A, B](as: List[A])(f: A => Option[B]): Option[List[B]] =
    as.foldRight(Option(List.empty[B]))((a, acc) => Monad[Option].map2(f(a), acc)(_ :: _))

  def sumF[F[_]: Monad](fa: F[Int], fb: F[Int]): F[Int] = Monad[F].map2(fa, fb)(_ + _)

  def main(args: Array[String]): Unit = {
    println(Monad[Option].map(Some(2))(_ * 3))
    println(Monad[List].flatMap(List(1, 2))(x => List(x, x * 10)))
    println(Monad[Option].map2(Some(1), Some(2))(_ + _))
    println(Monad[Option].map2(Some(1), None: Option[Int])(_ + _))
    println(sumF(Option(3), Option(4)))
    println(sumF(List(1, 2), List(10, 20)))
    println(traverseOpt(List("1", "2"))(s => scala.util.Try(s.toInt).toOption))
    println(traverseOpt(List("1", "x"))(s => scala.util.Try(s.toInt).toOption))
    println(Option(5).fmap(_ + 1))
    println(List(1, 2).bind(x => List(x, -x)))

    def compose[F[_]: Monad, A](fa: F[A], n: Int)(f: A => A): F[A] =
      (1 to n).foldLeft(fa)((acc, _) => Monad[F].map(acc)(f))
    println(compose(Option(1), 3)((x: Int) => x * 2))
    println(compose(List(1, 2), 2)((x: Int) => x + 1))
  }
}
