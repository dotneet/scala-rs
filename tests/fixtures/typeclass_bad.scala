trait Show[A] { def show(a: A): String }
object Main {
  def render[A](a: A)(implicit s: Show[A]): String = s.show(a)
  def f = render(3)
}
