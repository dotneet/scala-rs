// `agent/kindvar`: kinds of the type arguments of a *written* class type
// (`new Foo[Set]`, a parent, a member's declared type), one per line marked
// `// error` -- exactly the lines scalac 2.13.16 rejects. nsc defers this
// `checkBounds` to refchecks (`TypeTreeWithDeferredRefCheck`), so nothing in
// this file may fail in typer; the method type-argument cases, which typer
// reports, are in `kvar_kind_bad.scala`.
import scala.collection.{mutable => mu}

trait CoFunctor[F[+_]]
trait ContraFunctor[F[-_]]
class HK[M[_[_]]]
class AR[M[_[_]]]
trait Bi[F[_, _]]

object ClassArgs {
  class Foo[F[+_]]
  class Box[F[-_]]
  new Foo[Set] // error
  new Box[List] // error
  val v: Foo[mu.ArrayBuffer] = null // error
  new HK[CoFunctor] // error
  new AR[Bi] // error
  class Wrap2[G[_]] {
    def f: CoFunctor[G] = null // error
    def g: ContraFunctor[G] = null // error
  }
  trait Tc {
    type N[X]
    type K[-X]
    def b: CoFunctor[N] = null // error
    def d: CoFunctor[K] = null // error
  }
}
