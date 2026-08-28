object Main {
  def maxOf[T: Ordering](xs: List[T]): T = xs.max
  def sorted[T](xs: List[T])(implicit o: Ordering[T]): List[T] = xs.sorted
  def main(args: Array[String]): Unit = {
    println(maxOf(List(3, 1, 2)))
    println(sorted(List("b", "a")))
    println(Math.abs(-3))
    println(System.lineSeparator().length)
  }
}
