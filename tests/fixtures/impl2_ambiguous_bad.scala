// Two equally specific polymorphic derivation rules match the same type:
// nsc "ambiguous implicit values: both method boxA ... and method boxB".

trait Show[A] { def show(a: A): String }

final class Box[A](val value: A)

object Show {
  implicit val showInt: Show[Int] = new Show[Int] { def show(a: Int): String = a.toString }

  implicit def boxA[A](implicit s: Show[A]): Show[Box[A]] =
    new Show[Box[A]] { def show(b: Box[A]): String = "A" }
  implicit def boxB[A](implicit s: Show[A]): Show[Box[A]] =
    new Show[Box[A]] { def show(b: Box[A]): String = "B" }
}

object Main {
  def render[A](a: A)(implicit s: Show[A]): String = s.show(a)

  def main(args: Array[String]): Unit = {
    println(render(new Box(1)))
  }
}
