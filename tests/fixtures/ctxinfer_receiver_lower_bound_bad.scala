final class Layer[E, A](val value: A) {
  def flatMap[EE >: E, B](f: A => Layer[EE, B]): Layer[EE, B] = f(value)
}
object Main {
  val result: Layer[String, Int] =
    new Layer[Int, Int](1).flatMap(n => new Layer[String, Int](n + 1))
}
