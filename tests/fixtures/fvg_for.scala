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

  var fCalls = 0
  var gCalls = 0

  def f(x: Int): Int = {
    fCalls = fCalls + 1
    println("f" + x + ":" + fCalls)
    x + 10
  }

  def g(x: Int): Int = {
    gCalls = gCalls + 1
    println("g" + x + ":" + gCalls)
    x + 100
  }

  def evaluationOrder(xs: List[Int]): List[Int] =
    for (x <- xs; y = f(x); z = g(y)) yield z

  object Extractor {
    var unapplyCalls = 0

    def unapply(x: Int): Option[(Int, Int)] = {
      unapplyCalls = unapplyCalls + 1
      println("unapply" + x + ":" + unapplyCalls)
      Some((x, x + 1))
    }
  }

  def patternEvaluation(xs: List[Int]): List[Int] =
    for (x <- xs; Extractor(a, b) = x; if a >= 0) yield a + b

  def boundPatternValue(xs: List[(Int, Int)]): List[Int] =
    for (x <- xs; p @ (a, b) = x; if p._1 >= 0) yield a + b

  def emit(xs: List[Int]): Unit = xs.foreach(x => println("result" + x))

  emit(basic(-1 :: 2 :: Nil))
  calls = 0
  emit(multiple(-2 :: 0 :: 1 :: Nil))
  emit(afterGuard(-3 :: -2 :: -1 :: 0 :: 1 :: 2 :: 3 :: Nil))
  emit(pattern((-2, 1) :: (1, 1) :: (2, -1) :: Nil))
  emit(evaluationOrder(1 :: 2 :: Nil))
  emit(patternEvaluation(3 :: Nil))
  emit(boundPatternValue((1, 2) :: (-1, 3) :: Nil))
}
