// Type classes the way cats/slick shape them: a trait with a companion holding
// the low-priority instances, an explicit `implicitly`, an implicit conversion
// used for syntax, and instance priority between companion and local scope.
object Main {
  trait Show[A] { def show(a: A): String }

  trait LowPriorityShow {
    implicit def anyShow[A]: Show[A] = new Show[A] { def show(a: A) = "<" + a.toString + ">" }
  }

  object Show extends LowPriorityShow {
    implicit val intShow: Show[Int] = new Show[Int] { def show(a: Int) = "i" + a }
    implicit val strShow: Show[String] = new Show[String] { def show(a: String) = "\"" + a + "\"" }
    implicit def listShow[A](implicit ev: Show[A]): Show[List[A]] =
      new Show[List[A]] { def show(a: List[A]) = a.map(ev.show).mkString("[", ",", "]") }
    implicit def pairShow[A, B](implicit sa: Show[A], sb: Show[B]): Show[(A, B)] =
      new Show[(A, B)] { def show(p: (A, B)) = "(" + sa.show(p._1) + " " + sb.show(p._2) + ")" }
    def apply[A](implicit s: Show[A]): Show[A] = s
  }

  implicit class ShowOps[A](val a: A) extends AnyVal {
    def shown(implicit s: Show[A]): String = s.show(a)
  }

  case class Money(cents: Long)
  implicit val moneyShow: Show[Money] = new Show[Money] {
    def show(m: Money) = (m.cents / 100) + "." + f"${m.cents % 100}%02d"
  }

  class Cents(val n: Int)
  implicit def cents2money(c: Cents): Money = Money(c.n.toLong)

  def render[A: Show](a: A): String = implicitly[Show[A]].show(a)

  def main(args: Array[String]): Unit = {
    println(render(42))
    println(render("hi"))
    println(render(List(1, 2, 3)))
    println(render(List("a", "b")))
    println(render((1, "x")))
    println(render(Money(12345)))
    println(render(3.5))
    println(List(1, 2).shown)
    println(Show[Int].show(7))
    println(Show.apply[List[(Int, String)]].show(List((1, "a"), (2, "b"))))
    val c = new Cents(250)
    println(render(c: Money))
    println((c: Money).cents)
    def twice[A](a: A)(implicit s: Show[A]): String = s.show(a) + s.show(a)
    println(twice(1))
    println(twice(Money(5)))
  }
}
