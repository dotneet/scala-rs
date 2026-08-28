// A *view* bound on a higher-kinded type parameter stays illegal in 2.13.16:
//   error: type F takes type parameters
trait V
object Main {
  def f[F[_] <% V](x: Int): Int = x
  def main(args: Array[String]): Unit = println(f[List](1))
}
