final case class Item(value: Int)

object Main {
  def fromOption(value: Option[Int]): Item =
    value.fold(throw new Exception("empty"))(Item(_))

  def fromOptionInferred(value: Option[Int]) =
    value.fold(throw new Exception("empty"))(Item(_))

  def choose[A](empty: => A)(f: Int => A): A =
    Option(7).fold(empty)(f)

  def chosen: Item = choose(throw new Exception("empty"))(Item(_))

  def chosenInferred = choose(throw new Exception("empty"))(Item(_))

  def report(label: String)(value: => Item): Unit =
    try println(label + ":" + value.value)
    catch { case _: Exception => println(label + ":empty") }

  def main(args: Array[String]): Unit = {
    report("expected-some")(fromOption(Some(5)))
    report("inferred-some")(fromOptionInferred(Some(6)))
    report("expected-none")(fromOption(None: Option[Int]))
    report("inferred-none")(fromOptionInferred(None: Option[Int]))
    report("choose")(chosen)
    report("choose-inferred")(chosenInferred)
  }
}
