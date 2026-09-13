// A real Cats `Aux` consumer.  The scala-rs build used to pickle the fresh
// parameter in `type F[x] = F0[x]` as FINAL instead of PARAM, so nsc rejected
// the two alpha-equivalent refinements after `NonEmptyParallel[E]` expanded
// its dependent result `P.Aux[M, P.F]`.
import cats.{NonEmptyParallel, Parallel}
import cats.data.Validated
import cats.instances.either._

object Main {
  type E[A] = Either[String, A]
  type V[A] = Validated[String, A]

  def main(args: Array[String]): Unit = {
    val nonEmpty: NonEmptyParallel.Aux[E, V] = NonEmptyParallel[E]
    val parallel: Parallel.Aux[E, V] = Parallel[E]
    println(nonEmpty != null)
    println(parallel != null)
  }
}
