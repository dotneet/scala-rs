trait Show[A] { def show(a: A): String }
object Show {
  implicit val intShow: Show[Int] = new Show[Int] { def show(a: Int): String = "i" + a.toString }
  implicit val strShow: Show[String] = new Show[String] { def show(a: String): String = "s" + a }
}
class Box[A](val value: A) { def map[B](f: A => B): Box[B] = new Box(f(value)) }
object Main {
  def render[A](a: A)(implicit s: Show[A]): String = s.show(a)
  def firstOr[A](xs: List[A], d: A): A = xs.headOption.getOrElse(d)
  def main(args: Array[String]): Unit = {
    import Show._
    println(render(3))
    println(render("x"))
    println(new Box(2).map(_ + 1).value)
    println(firstOr(List(1, 2), 0))
    println(firstOr(List[Int](), 7))
    def maxOf[T: Ordering](xs: List[T]): T = xs.max
    println(maxOf(List(2, 9, 4)))
  }
}
