final case class Item(value: Int)

object Main {
  def choose[A](empty: => A)(f: Int => A): A =
    Option(7).fold(empty)(f)

  val badFold = Option(1).fold[Nothing](throw new Exception("empty"))(Item(_))
  val badChoose = choose[Nothing](throw new Exception("empty"))(Item(_))
}
