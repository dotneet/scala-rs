object Main extends App {
  object Stateful {
    var calls = 0

    def unapply(x: Int): Option[(Int, Int)] = {
      calls = calls + 1
      if (calls == 1) Some((x, x)) else None
    }
  }

  println(for (x <- 1 :: Nil; Stateful(a, b) = x; if a >= 0) yield a + b)
}
