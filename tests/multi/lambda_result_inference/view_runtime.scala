class Box[A](val value: A)
object Main {
  implicit def wrap[A](box: Box[A]): Box[Option[A]] = new Box(Some(box.value))
  def use[A](f: Int => Box[Option[A]]): Box[Option[A]] = f(1)
  def main(args: Array[String]): Unit = {
    val result = use(_ => new Box(42))
    println(result.value)
  }
}
