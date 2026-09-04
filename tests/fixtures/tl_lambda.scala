// Structural type lambdas: `({ type L[a] = … })#L` used as a type constructor.
//
// A *named* higher-kinded alias already worked; the projection out of a
// refinement did not, because every written refinement allocates its own
// symbol and two of them could never compare equal. The cases below are the
// ones cats writes when kind-projector is not on the classpath:
//
//   1. a structural lambda against a named alias for the same lambda,
//   2. a structural lambda against a plain class constructor,
//   3. a parent written with a lambda over the class's own type parameter,
//   4. an implicit whose result captures the method's own type parameter,
//      found by implicit search and passed as a type constructor argument.
object Main {
  trait Functor[F[_]] { def map[A, B](fa: F[A])(f: A => B): F[B] }
  trait Monad[F[_]] extends Functor[F] {
    def pure[A](a: A): F[A]
    def flatMap[A, B](fa: F[A])(f: A => F[B]): F[B]
    def map[A, B](fa: F[A])(f: A => B): F[B] = flatMap(fa)(a => pure(f(a)))
  }

  sealed abstract class Res[E, A]
  final case class Bad[E, A](e: E) extends Res[E, A]
  final case class Good[E, A](a: A) extends Res[E, A]

  final case class Reader[R, A](run: R => A)
  final case class Box[A](value: A)

  def show[E, A](r: Res[E, A]): String = r match {
    case Bad(e)  => "Bad(" + e + ")"
    case Good(a) => "Good(" + a + ")"
  }

  type ResS[a] = Res[String, a]

  // (1) the named alias and the structural lambda are one type, in both
  // directions.
  val named: Functor[ResS] = new Functor[({ type L[a] = Res[String, a] })#L] {
    def map[A, B](fa: Res[String, A])(f: A => B): Res[String, B] = fa match {
      case Bad(e)  => Bad(e)
      case Good(a) => Good(f(a))
    }
  }
  val structural: Functor[({ type L[a] = Res[String, a] })#L] = named

  // (2) the body is just a class constructor, so `Functor[Box]` fits.
  val boxF: Functor[({ type L[a] = Box[a] })#L] = new Functor[Box] {
    def map[A, B](fa: Box[A])(f: A => B): Box[B] = Box(f(fa.value))
  }

  // (3) the lambda captures the class's own type parameter, so instantiating
  // the class has to reach inside the lambda's body.
  class ResFunctor[E] extends Functor[({ type L[a] = Res[E, a] })#L] {
    def map[A, B](fa: Res[E, A])(f: A => B): Res[E, B] = fa match {
      case Bad(e)  => Bad(e)
      case Good(a) => Good(f(a))
    }
  }
  val captured: Functor[({ type L[a] = Res[Int, a] })#L] = new ResFunctor[Int]

  // (4) the same capture, on a method, reached through implicit search.
  implicit def readerMonad[R]: Monad[({ type L[X] = Reader[R, X] })#L] =
    new Monad[({ type L[X] = Reader[R, X] })#L] {
      def pure[A](a: A): Reader[R, A] = Reader(_ => a)
      def flatMap[A, B](fa: Reader[R, A])(f: A => Reader[R, B]): Reader[R, B] =
        Reader(r => f(fa.run(r)).run(r))
    }

  def twice[F[_]](fa: F[Int])(implicit F: Monad[F]): F[Int] =
    F.flatMap(fa)(i => F.pure(i * 2))

  // (5) cats' `Parallel.Aux` shape: a refinement that names a type
  // constructor member. `seq`'s `F` occurs nowhere but the implicit clause, so
  // the witness is the only thing that can pin it down -- and it can only do
  // so if the refinement's declaration really carries what `F0` was.
  trait Par[M[_]] {
    type F[_]
    def one[A](m: M[A]): F[A]
  }
  object Par {
    type Aux[M[_], F0[_]] = Par[M] { type F[x] = F0[x] }
    def seq[M[_], F[_], A](m: M[A])(implicit P: Par.Aux[M, F]): F[A] = P.one(m)
  }
  final class ParOps[M[_], A](val m: M[A]) {
    def go[F[_]](implicit P: Par.Aux[M, F]): F[A] = Par.seq(m)
  }
  implicit val boxPar: Par.Aux[Box, Box] = new Par[Box] {
    type F[x] = Box[x]
    def one[A](m: Box[A]): Box[A] = m
  }

  def main(args: Array[String]): Unit = {
    println(show(named.map(Good[String, Int](2))(_ + 1)))
    println(show(structural.map(Bad[String, Int]("no"))(_ + 1)))
    println(boxF.map(Box(21))(_ * 2).value)
    println(show(captured.map(Good[Int, Int](5))(_ + 1)))
    println(show(captured.map(Bad[Int, Int](7))(_ + 1)))
    val m = implicitly[Monad[({ type L[X] = Reader[Int, X] })#L]]
    println(m.flatMap(Reader[Int, Int](r => r + 1))(a => m.pure(a * 10)).run(4))
    println(m.map(Reader[Int, Int](r => r * 3))(_ + 1).run(5))
    println(twice[({ type L[X] = Reader[Int, X] })#L](Reader(r => r + 100)).run(1))
    println(new ParOps(Box(9)).go[Box].value)
  }
}
