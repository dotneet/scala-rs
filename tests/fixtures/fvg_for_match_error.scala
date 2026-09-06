object Main extends App {
  object Missing {
    def unapply(x: Int): Option[(Int, Int)] = None
  }

  println(for (x <- 1 :: Nil; Missing(a, b) = x) yield a + b)
}
