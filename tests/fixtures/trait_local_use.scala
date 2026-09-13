trait LocalAlternative[F[_]] extends LocalApplicative[F] with LocalSemigroupK[F]

object Main {
  def main(args: Array[String]): Unit = {
    val x = new LocalAlternative[List] {}
    println(x.runApplicative(List(1)))
    println(x.runSemigroupK(List(2)))
  }
}
