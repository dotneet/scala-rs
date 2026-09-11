// Programs next to `catsr_infer.scala` that scalac 2.13.16 rejects, one per
// line marked `// error`. Each rule that fixture relies on must still refuse
// these.

object CompoundBad {
  trait Functor[F[_]]; trait Monad[F[_]] extends Functor[F]
  trait Traverse[F[_]] extends Functor[F]; trait Align[F[_]]
  trait P[X]; trait Q[X] extends P[X]; trait R[X]
  def ta[A]: Traverse[({ type L[x] = Either[A, x] })#L] with Align[({ type L[x] = Either[A, x] })#L] = null
  def plainInt[A]: Q[List[Int]] with R[List[A]] = null
  def w[A]: R[A] with Q[List[A]] = null
  class Use[E] {
    val a: Monad[({ type L[x] = Either[E, x] })#L] = ta // error
    val b: P[List[E]] = plainInt // error
    val c: P[Set[E]] = w // error
  }
}

object LaterClauseBad {
  final class St[SA, SB](val run: SA => SB) {
    def dimap[S0, S1](f: S0 => SA)(g: SB => S1): St[S0, S1] = new St(s => g(run(f(s))))
  }
  def first[A, B, C](fa: St[A, B]): St[(A, C), (B, C)] = null
  def bad1[A, B, C](fa: St[A, B]): St[(C, A), (C, B)] =
    first(fa).dimap((_: (C, A)).swap)((x: (B, Int)) => x.swap) // error
  def bad2[A, B, C](fa: St[A, B]): St[(C, A), (B, C)] =
    first(fa).dimap((_: (C, A)).swap)(_.swap) // error
}

object ImplicitAlternativeBad {
  trait Applicative[F[_]] { def compose[G[_]: Applicative]: Applicative[({ type L[a] = F[G[a]] })#L] = null }
  trait MonoidK[F[_]] { def compose[G[_]]: MonoidK[({ type L[a] = F[G[a]] })#L] = null }
  trait Alternative[F[_]] extends Applicative[F] with MonoidK[F] {
    override def compose[G[_]: Applicative]: Alternative[({ type L[a] = F[G[a]] })#L] = null
  }
  def h[F[_], G[_]](A: Alternative[F]): MonoidK[({ type L[a] = F[G[a]] })#L] = A.compose[G] // error
}

object CurriedBad {
  def map2[B](f: (Boolean, Int) => B): B = f(true, 1)
  def ite(b: Boolean)(x: Int) = x
  def bad = map2(ite) // error
}

object ByNameBad {
  abstract class AT[-T, +R] extends (T => R)
  object AT { def fromFn[T, R](f: T => R): AT[T, R] = new AT[T, R] { def apply(t: T): R = f(t) } }
  final class Cont[M[_], A, +B](val runAndThen: AT[B => M[A], M[A]]) {
    def bad1: Cont[M, A, Int] = Cont.later(runAndThen) // error
    def bad2: Cont[M, A, B] = Cont.later(3) // error
  }
  object Cont { def later[M[_], A, B](fn: => (B => M[A]) => M[A]): Cont[M, A, B] = new Cont(AT.fromFn(fn)) }
}

object ShadowBad {
  trait Show[A] { def show(a: A): String }
  def h(implicit F: Show[Int]): String = {
    val F = 3
    implicitly[Show[Int]].show(F) // error
  }
}

object OuterBad {
  trait Conv[F[_], G[_]] { def apply[A](fa: F[A]): G[A] }
  trait Base[M[_]] { def conv: Conv[M, List]; def wrap[A](a: A): M[A] }
  trait Runner { def run: String }
  trait User[M[_]] extends Base[M] {
    def r: Runner = new Runner { def run: String = { val x: List[Int] = conv(wrap("s")); "" } } // error
    def r2: Runner = new Runner { def run: String = { val x: Option[String] = wrap("s"); "" } } // error
  }
  trait Box[A] {
    def get: A
    def mk: Box[Int] = new Box[Int] { def get = 1; def g: String = get } // error
  }
}

object ScrutineeBad {
  final case class SCF[A](a: A) extends Function1[Any, A] { def apply(arg: Any): A = a }
  def bad[B](run: Int => Option[B]): Int = run match {
    case SCF(fb) => val x: Int = fb; x // error
    case _ => 0
  }
}

object CaseApplyBad {
  final case class Kl[F[_], A, B](run: A => F[B])
  val bad1: Kl[Option, Either[Int, String], String] = Kl((fe: Int) => Option("x")) // error
  val bad2: Kl[Option, Int, String] = Kl((i: Int) => Option(i)) // error
  val a: Either[String, Int] = Right("s") // error
  val b: Either[String, Int] = Left(1) // error
}

object MethodValueBad {
  final case class Fn[A, B](f: A => B)
  def lift[A, B](f: A => B): Fn[A, B] = Fn(f)
  def twice(i: Int): Int = i * 2
  val bad1: Fn[String, Int] = lift(twice) // error
  val bad2: Fn[Int, String] = lift(identity[Int]) // error
}

object ArrowBad {
  trait Arrow[F[_, _]] {
    def compose[A, B, C](f: F[B, C], g: F[A, B]): F[A, C]
    def first[A, B, C](fa: F[A, B]): F[(A, C), (B, C)]
    def swap[X, Y]: F[(X, Y), (Y, X)]
    def bad1[A, B, C](fa: F[A, B]): F[(C, A), (B, C)] =
      compose(swap, compose(first[A, B, C](fa), swap)) // error
    def bad3[A, C](x: F[(A, C), Int]) = compose(x, swap) // error
  }
}
