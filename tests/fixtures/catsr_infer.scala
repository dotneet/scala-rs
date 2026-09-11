// Inference roots from typelevel/cats core (`tests/cats_measure.sh`), each
// reduced to a standalone program and checked against scalac 2.13.16.
// The object names say which cats source line the shape comes from.

// cats/instances/either.scala:244, data/EitherT.scala:1050 -- a compound
// result read against a base class of one of its components.
object CompoundResult {
  trait Functor[F[_]]
  trait Monad[F[_]] extends Functor[F]
  trait MonadError[F[_], E] extends Monad[F]
  trait Traverse[F[_]] extends Functor[F]
  trait P[X]; trait Q[X] extends P[X]; trait Q2[X] extends P[X]; trait R[X]
  class Inst[A] extends MonadError[({ type L[x] = Either[A, x] })#L, A] with Traverse[({ type L[x] = Either[A, x] })#L]
  def std[A]: MonadError[({ type L[x] = Either[A, x] })#L, A] with Traverse[({ type L[x] = Either[A, x] })#L] = new Inst[A]
  def z[A]: Q2[List[Int]] with Q[List[A]] = null
  def run(): String = {
    val m: Monad[({ type L[x] = Either[String, x] })#L] = std
    val p: P[List[String]] = z
    s"${m.getClass.getSimpleName.nonEmpty} ${p == null}"
  }
}

// cats/data/IndexedStateT.scala:444 -- the receiver's variable solved by the
// first clause reaches the second.
object LaterClause {
  final class St[SA, SB](val run: SA => SB) {
    def dimap[S0, S1](f: S0 => SA)(g: SB => S1): St[S0, S1] = new St(s => g(run(f(s))))
  }
  def first[A, B, C](fa: St[A, B]): St[(A, C), (B, C)] = new St({ case (a, c) => (fa.run(a), c) })
  def second[A, B, C](fa: St[A, B]): St[(C, A), (C, B)] = first(fa).dimap((_: (C, A)).swap)(_.swap)
  def run(): String = second[Int, String, Boolean](new St((i: Int) => "#" + i)).run((true, 41)).toString
}

// cats/data/Nested.scala:117 -- a type alias over a member val whose type is
// still inferred.
object PathHead {
  trait Rep { self =>
    type R
    def index: R => Int
    def compose(g: Rep): Rep.Aux[(self.R, g.R)] = new Rep {
      type R = (self.R, g.R)
      def index: R => Int = (r: R) => self.index(r._1) + g.index(r._2)
    }
  }
  object Rep {
    type Aux[X] = Rep { type R = X }
    val int: Aux[Int] = new Rep { type R = Int; def index: Int => Int = i => i * 10 }
    def nested(f0: Rep, g0: Rep): Aux[(f0.R, g0.R)] = new Rep {
      val FG = f0.compose(g0)
      type R = FG.R
      def index: R => Int = FG.index
    }
  }
  def run(): String = Rep.nested(Rep.int, Rep.int).index((1, 2)).toString
}

// cats/data/Nested.scala:162 -- `Alternative.compose[G: Applicative]` beside
// the inherited `MonoidK.compose[G]`.
object ImplicitAlternative {
  trait Ev[T]
  implicit val intEv: Ev[Int] = new Ev[Int] {}
  class R1 { override def toString = "R1" }; class R2 extends R1 { override def toString = "R2" }
  class Base { def m[T]: String = "nullary-Base" }
  class Sub extends Base { def m[T](implicit ev: Ev[T]): String = "implicit-Sub" }
  class C8 { def k[T]: R1 = new R1; def k[T](implicit ev: Ev[T]): R2 = new R2 }
  class C9 { def k[T]: R2 = new R2; def k[T](implicit ev: Ev[T]): R1 = new R1 }
  class U1 { override def toString = "U1" }
  class C7 { def k[T]: U1 = new U1; def k[T](implicit ev: Ev[T]): R1 = new R1 }
  def run(): String = {
    val a: R1 = new C7().k[Int]
    s"${new Sub().m[Int]} ${new C8().k[Int]} ${new C9().k[Int]} ${new C7().k[Int]} $a"
  }
}

// cats/Apply.scala:247 -- a curried method as a function argument.
object CurriedArgument {
  def map[A, B](fa: List[A])(f: A => B): List[B] = fa.map(f)
  def ifA[A](fcond: List[Boolean])(t: A, e: A): List[A] = {
    def ite(b: Boolean)(ifTrue: A, ifFalse: A) = if (b) ifTrue else ifFalse
    map(fcond)(ite).map(f => f(t, e))
  }
  def ov[B](f: Boolean => B): String = "one"
  def ov[B](f: (Boolean, Int) => B): String = "two"
  def ite(b: Boolean)(x: Int) = x
  def flat(b: Boolean, x: Int) = x
  def run(): String = s"${ifA(List(true, false))("x", "y")} ${ov(ite _)} ${ov(flat _)}"
}

// cats/data/ContT.scala:51 -- a by-name function parameter given a subclass
// of Function1.
object ByNameFunction {
  abstract class AT[-T, +R] extends (T => R) {
    override def andThen[A](g: R => A): AT[T, A] = AT.fromFn(g.compose(this))
  }
  object AT { def fromFn[T, R](f: T => R): AT[T, R] = new AT[T, R] { def apply(t: T): R = f(t) } }
  final class Cont[M[_], A, +B](val runAndThen: AT[B => M[A], M[A]]) {
    def mapCont(fn: M[A] => M[A]): Cont[M, A, B] = Cont.later(runAndThen.andThen(fn))
  }
  object Cont {
    def later[M[_], A, B](fn: => (B => M[A]) => M[A]): Cont[M, A, B] = new Cont(AT.fromFn(fn))
  }
  def run(): String = {
    val c = new Cont[List, Int, String](AT.fromFn((k: String => List[Int]) => k("ab")))
    c.mapCont(_.map(_ + 1)).runAndThen((s: String) => List(s.length)).toString
  }
}

// cats/data/IorT.scala:609 -- a type member named like an implicit parameter.
object TypeDoesNotShadow {
  trait Show[A] { def show(a: A): String }
  trait G { def g: String }
  def f(implicit F: Show[Int]): String = new G { type F = Int; def g = implicitly[Show[Int]].show(1) }.g
  def run(): String = f(new Show[Int] { def show(a: Int) = "shown " + a })
}

// cats/Parallel.scala:106 -- an outer member read inside an anonymous class.
object OuterMember {
  trait Conv[F[_], G[_]] { def apply[A](fa: F[A]): G[A] }
  trait Base[M[_]] { def conv: Conv[M, List]; def wrap[A](a: A): M[A] }
  trait Runner { def run: String }
  class OptBase extends Base[Option] {
    def conv: Conv[Option, List] = new Conv[Option, List] { def apply[A](fa: Option[A]): List[A] = fa.toList }
    def wrap[A](a: A): Option[A] = Some(a)
  }
  trait User[M[_]] extends Base[M] {
    def runner[A](a: A): Runner = new Runner { def run: String = conv(wrap(a)).toString }
  }
  def run(): String = (new OptBase with User[Option]).runner("hi").run
}

// cats/data/Kleisli.scala:74 and :79 -- a function-typed scrutinee.
object FunctionScrutinee {
  final case class SCF[A](a: A) extends Function1[Any, A] { def apply(arg: Any): A = a }
  final case class Kl[F[_], -A, B](run: A => F[B]) {
    def peek: F[B] = run match {
      case SCF(fb) => fb
      case _ => sys.error("not constant")
    }
    def peek2: F[B] = run match {
      case r: SCF[_] => r.a
      case _ => sys.error("not constant")
    }
  }
  def run(): String = {
    val k = Kl[Option, Int, String](SCF(Some("x")))
    s"${k.peek} ${k.peek2}"
  }
}

// cats/data/Kleisli.scala:571 and data/EitherK.scala:60 -- the expected type
// reaches a case-class `apply`'s argument, an annotated literal's body, and
// `Left`/`Right`.
object CaseApplyPrototype {
  trait Functor[F[_]] { def map[A, B](fa: F[A])(f: A => B): F[B] }
  implicit val optF: Functor[Option] = new Functor[Option] { def map[A, B](fa: Option[A])(f: A => B): Option[B] = fa.map(f) }
  final case class Kl[F[_], A, B](run: A => F[B])
  def choose[F[_], A, B, C, D](f: A => F[C], g: B => F[D])(implicit F: Functor[F]): Kl[F, Either[A, B], Either[C, D]] =
    Kl((fe: Either[A, B]) =>
      fe match {
        case Left(a)  => F.map(f(a))(Left.apply _)
        case Right(b) => F.map(g(b))(Right.apply _)
      })
  final case class EK[F[_], G[_], A](run: Either[F[A], G[A]])
  def rightc[F[_], G[_], A](x: G[A]): EK[F, G, A] = EK(Right(x))
  def wrap[F[_], G[_], A](ga: G[A], G: Functor[G]): Either[F[EK[F, G, A]], G[EK[F, G, A]]] =
    Right(G.map(ga)(x => rightc(ga)))
  def run(): String = {
    val k = choose[Option, Int, String, String, Int]((i: Int) => Some("n" + i), (s: String) => Some(s.length))
    val e: Either[String, Int] = Right(4)
    s"${k.run(Left(4))} ${k.run(Right("abc"))} $e ${wrap[List, Option, Int](Some(1), optF)}"
  }
}

// cats/arrow/ArrowChoice.scala:58 and syntax/either.scala:332 -- method values.
object MethodValues {
  final case class Fn[A, B](f: A => B)
  def lift[A, B](f: A => B): Fn[A, B] = Fn(f)
  def twice(i: Int): Int = i * 2
  def one[A](a: A): List[A] = List(a)
  def mapL[A, B](xs: List[A])(f: A => B): List[B] = xs.map(f)
  def pair[A, B](a: A, b: B): (A, B) = (a, b)
  def zipWith[A, B, C](xs: List[A], ys: List[B])(f: (A, B) => C): List[C] = xs.zip(ys).map { case (a, b) => f(a, b) }
  def ap[AA, BB, C](eab: Either[AA, BB], that: Either[AA, BB => C]): Either[AA, C] = that.flatMap(eab.map)
  def run(): String = {
    val a = lift(identity[String])
    val g: Fn[Int, List[Int]] = lift(one)
    s"${a.f("id")} ${lift(twice).f(21)} ${g.f(7)} ${mapL(List(1, 2))(one)} ${zipWith(List(1), List("a"))(pair)} " +
      ap(Right(2): Either[String, Int], Right((i: Int) => "v" + i))
  }
}

// cats/arrow/Arrow.scala:46 -- a polymorphic nullary argument solved through
// the expected type, twice nested.
object ArrowSecond {
  trait Arrow[F[_, _]] {
    def lift[A, B](f: A => B): F[A, B]
    def compose[A, B, C](f: F[B, C], g: F[A, B]): F[A, C]
    def first[A, B, C](fa: F[A, B]): F[(A, C), (B, C)]
    def second[A, B, C](fa: F[A, B]): F[(C, A), (C, B)] = {
      def swap[X, Y]: F[(X, Y), (Y, X)] = lift[(X, Y), (Y, X)] { case (x, y) => (y, x) }
      compose(swap, compose(first[A, B, C](fa), swap))
    }
  }
  object FnArrow extends Arrow[Function1] {
    def lift[A, B](f: A => B): A => B = f
    def compose[A, B, C](f: B => C, g: A => B): A => C = a => f(g(a))
    def first[A, B, C](fa: A => B): ((A, C)) => (B, C) = { case (a, c) => (fa(a), c) }
  }
  def run(): String = FnArrow.second[Int, String, Boolean]((i: Int) => "#" + i)((true, 7)).toString
}

// cats/conversions/VarianceConversions.scala:28 -- a lenient prototype must
// not leak its wildcard into the argument.
object LenientPrototype {
  trait Functor[F[_]] { def widen[A, B >: A](fa: F[A]): F[B] = fa.asInstanceOf[F[B]] }
  trait Bifunctor[F[_, _]] {
    def rightFunctor[X]: Functor[({ type L[y] = F[X, y] })#L] = new Functor[({ type L[y] = F[X, y] })#L] {}
    def leftWiden[A, B, AA >: A](fab: F[A, B]): F[AA, B] = fab.asInstanceOf[F[AA, B]]
  }
  implicit val eitherBi: Bifunctor[Either] = new Bifunctor[Either] {}
  def widenBoth[F[_, _], A, B >: A, C, D >: C](fac: F[A, C])(implicit F: Bifunctor[F]): F[B, D] =
    F.leftWiden(F.rightFunctor.widen(fac))
  def run(): String = {
    val e: Either[Int, String] = Right("s")
    widenBoth[Either, Int, Any, String, CharSequence](e).toString
  }
}

object Main {
  def main(args: Array[String]): Unit = {
    println(CompoundResult.run())
    println(LaterClause.run())
    println(PathHead.run())
    println(ImplicitAlternative.run())
    println(CurriedArgument.run())
    println(ByNameFunction.run())
    println(TypeDoesNotShadow.run())
    println(OuterMember.run())
    println(FunctionScrutinee.run())
    println(CaseApplyPrototype.run())
    println(MethodValues.run())
    println(ArrowSecond.run())
    println(LenientPrototype.run())
  }
}
