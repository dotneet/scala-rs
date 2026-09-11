// Lambda results, negatives: a body that decides the result is still
// checked against what the declaration then requires.
object Main {
  def ap[A, B](a: A)(f: A => B): B = f(a)
  class Bx[A](val a: A)
  val bad1: List[String] = List(1).map(x => x)              // line 6
  val bad2: Bx[String] = ap(1)(x => new Bx(x))              // line 7
  val bad3: List[Int] = List(1, 2).map(x => if (x > 1) 1L else 0) // line 8
  def main(args: Array[String]): Unit = ()
}
