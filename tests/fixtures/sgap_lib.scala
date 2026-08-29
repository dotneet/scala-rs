object Main {
  // item 4b: `Iterable.apply` (scala-library only: the real jar's
  // `IterableFactory$Delegate.apply`; the private runtime has no backing
  // for a bare `Iterable` factory, so this needs --scala-library).
  def main(args: Array[String]): Unit = {
    val xs = Iterable("a", "b", "c")
    println(xs)
    println(xs.size)
  }
}
