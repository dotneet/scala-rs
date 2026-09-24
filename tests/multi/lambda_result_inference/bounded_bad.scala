class Box[A](val value: A)
object Main {
  def use[A <: CharSequence](f: Int => Box[A]): Box[A] = f(1)
  val result = use[Int](_ => new Box(42))
}
