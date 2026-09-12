// `agent/kindvar`, round 2: every line is accepted by scalac. `X`, `Inv`,
// `Contra` and `EitherT` come from `kvar_sep_lib.scala`'s class files, whose
// shallow `-cp` signature has no variance; the variance and kind checks must
// read the pickle before judging (`pos/t8708`).
trait Y[+B] {
  def m: X[B] = null
  type D[+A] = X[A]
  type E <: X[B]
}
trait ClientTypes[M[+_]] {
  final type Context[+A] = EitherT[M, String, A]
  final type StatefulContext[+A] = EitherT[Context, String, A]
}
object Main {
  def up[F[+_]](x: F[Int]): F[Any] = x
  def down[F[-_]](x: F[Int]): F[Nothing] = x
  def inv[F[_]](x: F[Int]): F[Int] = x
  def main(args: Array[String]): Unit = {
    println(up[X](new X[Int]) ne null)
    println(up(new X[Int]) ne null)
    println(down[Contra](new Contra[Int]) ne null)
    println(inv[Inv](new Inv[Int]) ne null)
    println(inv[X](new X[Int]) ne null)
  }
}
