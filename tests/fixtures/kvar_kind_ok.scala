// `agent/kindvar`: higher-kinded type arguments that *do* conform to the
// kinds of their parameters, and type lambdas whose parameters occur at their
// declared variance. Every line here is accepted by scalac 2.13.16; the kind
// check (`kind_bounds.rs`) must not refuse any of them. Each group prints
// something so a wrong refusal is visible as a missing line.
import scala.collection.{immutable => im, mutable => mu}
import scala.annotation.unchecked.uncheckedVariance
import scala.annotation.unchecked.{uncheckedVariance => uv}

// slick's `HCons`: `@uncheckedVariance` through a renaming import waives the
// class parameters' invariant position in a member alias.
final class HC[+H, +T](val head: H, val tail: T) {
  type Self = HC[H @uv, T @uv]
  type Head = H @uv
  type Tail = T @uncheckedVariance
}

trait Functor[F[_]] { def name: String }
trait CoFunctor[F[+_]] { def name: String }
trait ContraFunctor[F[-_]] { def name: String }
class HK[M[_[_]]](val tag: String)
class HK2[M[_[+_]]](val tag: String)

object Main {
  // --- a covariant, a contravariant and an invariant constructor parameter --
  def cov[F[+_]](x: F[Int]): F[Any] = x
  def contra[F[-_]](x: F[Int]): F[Nothing] = x
  def inv[F[_]](x: F[Int]): F[Int] = x
  def fn[F[-_, +_]](x: F[Int, Int]): F[Nothing, Any] = x
  def cov2[F[+_, +_]](x: F[Int, Int]): F[Any, Any] = x
  def mp[F[_, +_]](x: F[Int, Int]): F[Int, Any] = x
  def fn3[F[-_, -_, +_]](x: F[Int, Int, Int]): F[Nothing, Nothing, Any] = x

  // --- library classes at their declared variance (pickles) ---------------
  type Fn[-A] = A => Int
  type EC[+A] = Either[String, A]
  type Li[+A] = List[A]
  type Pl[A] = List[A]

  // --- abstract members as constructors --------------------------------
  trait Tc {
    type M[+X]
    type N[X]
    type K[-X]
    def a: Functor[M] = new Functor[M] { def name = "Functor[M]" }
    def b: Functor[N] = new Functor[N] { def name = "Functor[N]" }
    def c: CoFunctor[M] = new CoFunctor[M] { def name = "CoFunctor[M]" }
    def d: Functor[K] = new Functor[K] { def name = "Functor[K]" }
    def e: ContraFunctor[K] = new ContraFunctor[K] { def name = "ContraFunctor[K]" }
    def g(m: M[Int]): M[Any] = cov(m)
    def h(k: K[Int]): K[Nothing] = contra(k)
  }
  class Wrap[G[+_]] {
    def w: CoFunctor[G] = new CoFunctor[G] { def name = "CoFunctor[G]" }
    def f: Functor[G] = new Functor[G] { def name = "Functor[G]" }
    def pass(x: G[Int]): G[Any] = cov[G](x)
    def infer(x: G[Int]): G[Any] = cov(x)
  }

  // --- bounds of the constructor's own parameters ---------------------------
  class Num[A <: AnyVal](val a: A)
  class Bnd[A <: Int](val a: A)
  class Lo[A >: Null](val a: A)
  def bInt[F[_ <: Int]](x: F[Int]): F[Int] = x
  def bAnyVal[F[_ <: AnyVal]](x: F[Int]): F[Int] = x
  def bNull[F[_ >: Null]](x: F[Null]): F[Null] = x
  def bString[F[_ >: String]](x: F[String]): F[String] = x

  def main(args: Array[String]): Unit = {
    println(cov[List](List(1)).size)
    println(cov(List(1, 2)).size)
    println(cov[Option](Some(3)).get)
    println(cov[Vector](Vector(1, 2, 3)).size)
    println(cov(Iterable(1)).size)
    println(cov(Iterator(1, 2)).size)
    println(cov(scala.util.Try(4)).get)
    println(contra[Fn]((x: Int) => x + 1) eq null)
    println(inv[Set](Set(5)).size)
    println(inv[im.Set](Set(5)).size)
    println(inv[mu.ArrayBuffer](mu.ArrayBuffer(6)).size)
    println(inv(mu.ArrayBuffer(6, 7)).size)
    println(inv[List](List(8)).size)
    println(inv[java.util.ArrayList](new java.util.ArrayList[Int]).size)
    println(inv[Option](Some(9)).get)
    println(fn[Function1]((x: Int) => x * 2) ne null)
    println(fn[PartialFunction]({ case x => x }) ne null)
    println(fn3[Function2]((a: Int, b: Int) => a + b) ne null)
    println(cov2[Either](Right(12)).isRight)
    println(cov2[Tuple2]((13, 14))._1)
    println(mp[Map](Map(15 -> 16)).size)
    println(mp(Map(17 -> 18)).size)
    println(cov[EC](Right(19)).isRight)
    println(cov[Li](List(20)).size)
    println(inv[Pl](List(21)).size)
    println(contra[({ type L[-a] = a => Int })#L]((x: Int) => x) eq null)
    println(cov[({ type L[+a] = Either[String, a] })#L](Right(22)).isRight)
    println(inv[({ type L[a] = Either[String, a] })#L](Right(23)).isRight)
    println(cov[({ type L[+a] = Int })#L](24))
    println(contra[({ type L[-a] = Int })#L](25))
    println(inv[({ type L[a] = Int })#L](26))
    // `Any` and `Nothing` are kind-overloaded.
    def k[F[_]]: Int = 27
    def kc[F[+_]]: Int = 28
    println(k[Nothing] + kc[Nothing] + k[Any])
    // Nested constructors: `M[_[_]]` takes any first-order constructor, and
    // `M[_[+_]]` both an invariant and a covariant one.
    println(new HK[Functor]("HK[Functor]").tag)
    println(new HK2[Functor]("HK2[Functor]").tag)
    println(new HK2[CoFunctor]("HK2[CoFunctor]").tag)
    val tc = new Tc { type M[+X] = List[X]; type N[X] = Set[X]; type K[-X] = X => Int }
    println(tc.a.name + " " + tc.b.name + " " + tc.c.name + " " + tc.d.name + " " + tc.e.name)
    println(tc.g(List(29)).size)
    println(tc.h((x: Int) => x) eq null)
    val w = new Wrap[List]
    println(w.w.name + " " + w.f.name + " " + w.pass(List(30)).size + " " + w.infer(List(31)).size)
    println(bInt[Num](new Num(32)).a)
    println(bInt[Bnd](new Bnd(33)).a)
    println(bInt[List](List(34)).size)
    println(bAnyVal[Num](new Num(35)).a)
    println(bNull[Lo](new Lo[Null](null)).a)
    println(bString[Lo](new Lo[String]("36")).a)
    println(bNull[List](List(null)).size)
    // A type lambda's parameters at their declared variance.
    def foo[M[_]]: Int = 37
    def foo2[M[_, _]]: Int = 38
    println(foo[({ type l[+a] = List[a] })#l] + foo[({ type l[-a] = a => Int })#l])
    println(foo[({ type l[-a] = List[Int] })#l] + foo2[({ type l[+a, -b] = b => a })#l])
    println(foo[({ type l[+a] = (a => Int) => Int })#l] + foo[({ type l[-a] = a => Int })#l])
    println(foo[({ type l[+a] = List[a @uncheckedVariance] })#l])
    // A member's own parameter in its lower bound keeps its position.
    class Foo2[F[+_]] { type G[+x] >: F[x] }
    class Foo3 { type G[+x] <: List[x]; type H[+x] >: List[x] <: Any; type I[-x] = x => Int }
    println(new Foo2[List] != null)
    println(new Foo3 != null)
  }
}
