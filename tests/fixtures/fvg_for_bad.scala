object Main {
  def bad(xs: List[Int]): List[Int] = for {
    y = 1
    x <- xs
  } yield x + y
}
