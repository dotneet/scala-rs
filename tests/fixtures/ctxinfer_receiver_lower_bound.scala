final class Layer[E, A](val value: A) {
  def flatMap[EE >: E, B](f: A => Layer[EE, B]): Layer[EE, B] = f(value)
  def map[B](f: A => B): Layer[E, B] = new Layer(f(value))
}
object Layer {
  def lift[E, A](value: A): Layer[E, A] = new Layer(value)
}
object Main {
  def result: Layer[String, Int] = for {
    x <- Layer.lift(1)
    y <- new Layer[String, Int](x + 1)
  } yield y
  def retain[E](value: Layer[E, Int]): Layer[E, Int] =
    value.flatMap(n => new Layer[E, Int](n + 1))
  def main(args: Array[String]): Unit = {
    println(result.value)
    println(retain(result).value)
  }
}
