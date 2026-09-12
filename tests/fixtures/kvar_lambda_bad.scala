// `agent/kindvar`: type lambdas and higher-kinded type members whose own
// parameters occur at the wrong variance, and class parameters in a type
// member, one per line marked `// error` -- exactly the lines scalac 2.13.16
// rejects (nsc `validateVariance`, reported from refchecks, so nothing in this
// file may fail in typer).
import scala.annotation.unchecked.uncheckedVariance

trait Cov[+A]
trait Inv[-A]
class Foo[M[_]]

object LambdaBad {
  def up[F[+_]](fa: F[String]): F[Object] = fa
  def down[F[-_]](fa: F[Object]): F[String] = fa
  def foo[M[_]]: Int = 1
  type Stringer[-A] = A => String

  // `neg/t7872b`
  def oops1 = down[({ type l[-a] = List[a] })#l](List('whatever: Object)).head + "oops" // error
  def oops2 = up[({ type l[+a] = Stringer[a] })#l]("printed: " + _) // error
  // in every type position the refinement can be written in
  val v: Foo[({ type l[+a] = Inv[a] })#l] = null // error
  def p(x: Foo[({ type l[+a] = Inv[a] })#l]): Int = 1 // error
  new Foo[({ type l[-a] = Cov[a] })#l] // error
  foo[({ type l[-a] = (a => Int) => Int })#l] // error
  foo[({ type l[+a] = Cov[a] => Int })#l] // error
  foo[({ type l[+a] = Array[a] })#l] // error
  foo[({ type l[+a] = Inv[Cov[a]] })#l] // error
  def h[G[_]](x: G[String]) = up[({ type L[+a] = G[a] })#L](x) // error
  // named aliases and abstract members (`neg/t7872`)
  type l[-a] = Cov[a] // error
  type m[+a] = Inv[a] // error
  type x = { type l[-a] = Cov[a] } // error
  type G[-x] <: List[x] // error
  type H[-x] >: List[x] // error
  // a definition local to a block is exempt
  def local = { type ok[-a] = Cov[a]; 1 }
}

class X extends Foo[({ type l[+a] = Inv[a] })#l] // error

class C[+T] { type l[-a] = Cov[T] } // error
class D[+T] { type U <: List[T] }
class E[+T] { type U >: List[T] } // error
class G[+T] { type V = List[T] } // error
class H[+T] { private[this] type V = List[T] }
