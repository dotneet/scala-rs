trait LocalApplicative[F[_]] {
  def runApplicative(fa: F[Int]): F[Int] = {
    def loop(fa: F[Int], n: Int, acc: F[Int]): F[Int] =
      if (n == 0) acc else loop(fa, n - 1, acc)
    loop(fa, 1, fa)
  }
}
