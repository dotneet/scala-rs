trait Tag[A]
class Box[A](val value: A)
object Box {
  implicit val boxTag: Tag[Box[Int]] = new Tag[Box[Int]] {}
}
object Outer {
  class Inner
  object Inner {
    implicit val innerTag: Tag[Inner] = new Tag[Inner] {}
  }
}
object Main {
  def use[A](x: A)(implicit t: Tag[A]): String = "ok"
  def main(args: Array[String]): Unit = {
    println(use(new Outer.Inner()))
    println(use(new Box(1)))
  }
}
