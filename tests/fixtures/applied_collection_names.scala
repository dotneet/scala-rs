package custom {
  class List[A](val value: A)
  class Option[A](val value: A)
  class Some[A](val value: A)
}
object Main {
  def main(args: Array[String]): Unit = {
    import custom.{List, Option, Some}
    val list: custom.List[Int] = new List[Int](7)
    val option: custom.Option[String] = new Option[String]("option")
    val some: custom.Some[Int] = new Some[Int](9)
    println(list.value)
    println(option.value)
    println(some.value)
    val standard: scala.collection.immutable.List[Int] = scala.collection.immutable.List(1, 2)
    println(standard.sum)
  }
}
