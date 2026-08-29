// `loop` matches every type and asks for the very same type again. The search
// must cut off instead of looping: nsc "diverging implicit expansion for type
// Show[Int] starting with method loop".

trait Show[A] { def show(a: A): String }

object Main {
  implicit def loop[A](implicit a: A): A = a

  def render[A](a: A)(implicit s: Show[A]): String = s.show(a)

  def main(args: Array[String]): Unit = {
    println(render(1))
  }
}
