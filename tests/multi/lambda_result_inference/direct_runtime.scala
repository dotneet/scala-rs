class Box[A](val value: A)
object Main {
  implicit def convert[A](box: Box[A]): Box[String] = new Box("converted")
  def use[A](f: Int => Box[A]): Box[A] = f(1)
  def main(args: Array[String]): Unit = {
    val result = use(_ => new Box(42))
    println(result.value)
  }
}
