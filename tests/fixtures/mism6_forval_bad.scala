// nsc pairs a for-comprehension's value definition up with the generator's
// element and filters the resulting stream. This desugaring puts the value in
// a `val` inside the lambda's body, which has no stream to filter, so a guard
// that follows one is diagnosed rather than mis-compiled. nsc accepts it.

object Main {
  def bad(ms: List[Int]): List[Int] = for {
    m <- ms
    q = m + 1
    if q > 0
  } yield q

  def main(args: Array[String]): Unit = println(bad(1 :: Nil))
}
