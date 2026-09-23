// fs2's `Compiler` shape: `target[F]: Compiler[F, F]` in a higher-priority
// trait, `resource[F]: Compiler[F, Resource[F, *]]` below it. Neither result
// type is as specific as the other (F occurs twice in `target`), so the
// inheritance order alone picks `target` for an undetermined `G`.
trait Sync[F[_]]
class IO[A]
object IO { implicit val syncIO: Sync[IO] = new Sync[IO] {} }
class Resource[F[_], A]

trait Compiler[F[_], G[_]] { def name: String }
object Compiler extends CompilerLowPriority {
  trait Target[F[_]]
  object Target {
    implicit def forSync[F[_]](implicit F: Sync[F]): Target[F] = new Target[F] {}
  }
}
trait CompilerLowPriority extends CompilerLowPriority1
trait CompilerLowPriority1 extends CompilerLowPriority2 {
  implicit def target[F[_]](implicit F: Compiler.Target[F]): Compiler[F, F] =
    new Compiler[F, F] { def name = "target" }
}
trait CompilerLowPriority2 {
  type R[F[_]] = { type L[A] = Resource[F, A] }
  implicit def resource[F[_]](implicit F: Compiler.Target[F]): Compiler[F, R[F]#L] =
    new Compiler[F, R[F]#L] { def name = "resource" }
}

object IOCompilerPriority {
  def compile[G[_]](implicit c: Compiler[IO, G]): String = c.name
  def main(args: Array[String]): Unit = println(compile)
}
