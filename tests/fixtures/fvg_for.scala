object Main extends App {
  var calls = 0

  def mark(x: Int): Int = {
    calls = calls + 1
    println("rhs" + x + ":" + calls)
    x
  }

  def basic(xs: List[Int]): List[Int] =
    for (x <- xs; y = mark(x); if y > 0) yield y

  def multiple(xs: List[Int]): List[Int] =
    for (x <- xs; y = x + 1; z = y * 2; if z > 0) yield z

  def afterGuard(xs: List[Int]): List[Int] =
    for (x <- xs; y = x + 1; if y > 0; z = y * 2; if z < 10) yield z

  def pattern(xs: List[(Int, Int)]): List[Int] =
    for (p @ (a, b) <- xs; c = a + b; if c > 0) yield p._1 + c

  def emit(xs: List[Int]): Unit = xs.foreach(x => println("result" + x))

  emit(basic(-1 :: 2 :: Nil))
  calls = 0
  emit(multiple(-2 :: 0 :: 1 :: Nil))
  emit(afterGuard(-3 :: -2 :: -1 :: 0 :: 1 :: 2 :: 3 :: Nil))
  emit(pattern((-2, 1) :: (1, 1) :: (2, -1) :: Nil))
}
