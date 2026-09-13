trait LocalSemigroupK[F[_]] {
  def runSemigroupK(fa: F[Int]): F[Int] = {
    def loop(fa: F[Int], n: Int, extra: F[Int]): F[Int] =
      if (n == 0) extra else loop(fa, n - 1, extra)
    loop(fa, 1, fa)
  }
}
