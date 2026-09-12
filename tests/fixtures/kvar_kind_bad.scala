// `agent/kindvar`: type arguments whose kinds do not conform to their
// parameters, one per line marked `// error` -- exactly the lines scalac
// 2.13.16 rejects (nsc `checkKindBounds`, reported from typer). Only kind
// errors here: a typer error stops scalac before refchecks, so the type-lambda
// variance errors live in `kvar_lambda_bad.scala`.
import scala.collection.{immutable => im, mutable => mu}

trait Functor[F[_]]
trait CoFunctor[F[+_]]
trait ContraFunctor[F[-_]]
class HK[M[_[_]]]
class AR[M[_[_]]]
trait Bi[F[_, _]]

object KindBad {
  def up[F[+_]](fa: F[String]): F[Object] = fa
  def down[F[-_]](fa: F[Object]): F[String] = fa
  def cov[F[+_]](x: F[Int]): F[Any] = x
  def contra[F[-_]](x: F[Int]): F[Nothing] = x
  def inv[F[_]](x: F[Int]): F[Int] = x
  def fn[F[-_, +_]](x: F[Int, Int]): F[Nothing, Any] = x
  def cov2[F[+_, +_]](x: F[Int, Int]): F[Any, Any] = x
  type Stringer[-A] = A => String
  type EA[A] = Either[String, A]
  type Pl[A] = List[A]

  // inferred: `F := List` for `F[-_]`
  down(List('whatever: Object)) // error
  up(Set("a")) // error
  // written
  down[List](List('a: Object)) // error
  up[Stringer]("printed: " + _) // error
  up[java.util.List](new java.util.ArrayList[String]) // error
  cov[Set](Set(1)) // error
  cov[mu.ArrayBuffer](mu.ArrayBuffer(1)) // error
  cov[Ordering](null) // error
  cov[Array](Array(1)) // error
  contra[Option](None) // error
  fn[Either](Right(1)) // error
  fn[Tuple2]((1, 1)) // error
  cov2[Function1]((x: Int) => x) // error
  cov2[Map](Map(1 -> 1)) // error
  cov[EA](Right(1)) // error
  cov[Pl](List(1)) // error
  cov[({ type L[a] = Either[String, a] })#L](Right(1)) // error
  cov[({ type L[a] = Int })#L](1) // error
  // a constructor parameter handed on at the wrong variance
  def outer[G[_]](g: G[Int]) = cov[G](g) // error
  def outer2[G[_]](g: G[Int]) = cov(g) // error
  // bounds of the argument's own parameter stricter than the expectation's
  class Bnd[A <: AnyVal]
  class Lo2[A >: String]
  def bInt[F[_ <: Int]](x: F[Int]): F[Int] = x
  def bNull[F[_ >: Null]](x: F[Null]): F[Null] = x
  def bAnyVal[F[_ <: AnyVal]](x: F[Int]): F[Int] = x
  inv[Bnd](new Bnd[Int]) // error
  bNull[Lo2](null) // error
  class Bnd2[A <: Int]
  bAnyVal[Bnd2](new Bnd2[Int]) // error
}

object Inferred {
  trait Tc {
    type N[X]
    def h(n: N[Int]) = KindBad.cov(n) // error
  }
}
