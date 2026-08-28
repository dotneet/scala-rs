trait Show[A] { def show(a: A): String }
object Show {
  implicit val intShow: Show[Int] = new Show[Int] { def show(a: Int): String = "i" + a }
  implicit val strShow: Show[String] = new Show[String] { def show(a: String): String = "s" + a }
}
object Main {
  def render[A](a: A)(implicit s: Show[A]): String = s.show(a)
  def main(args: Array[String]): Unit = {
    import Show._
    println(render(3))
    println(render("x"))
  }
}
