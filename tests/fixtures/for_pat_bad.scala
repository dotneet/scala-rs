object Main {
  def f(xs: List[(Int, String)]): List[Int] = for ((n, s) <- xs) yield s.nosuchmember
}
