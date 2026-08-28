object Main {
  def f(xs: List[Int]): Int = xs match {
    case h :: t => h.nosuch
    case Nil => 0
  }
}
