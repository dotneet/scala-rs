package nestedbridge

trait R[F[_]] {
  def compose[G[_]](g: R[G]): R[({ type L[x] = F[G[x]] })#L] =
    new R[({ type L[x] = F[G[x]] })#L] {}
}

trait N[F[_]] extends R[F] {
  def compose[G[_]](g: N[G]): N[({ type L[x] = F[G[x]] })#L] =
    new N[({ type L[x] = F[G[x]] })#L] {}
}

object R {
  implicit val listR: R[List] = new R[List] {}
}

abstract class NR[F[_], G[_]](implicit G: R[G]) extends R[F]

class C[F[_]] extends NR[F, List] with N[F]

class D[F[_]] extends NR[F, List]

object Main {
  def main(args: Array[String]): Unit = {
    val d: R[List] = new D[List]
    println((new C[List]: R[List]).compose(d).getClass.getName)
  }
}
