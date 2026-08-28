object Main {
  def upcast[A, B](x: A)(implicit ev: A <:< B): B = ev(x)
  def sameType[A, B](x: A)(implicit ev: A =:= B): B = ev(x)

  def sumAll(xs: Iterable[Int]): Int = {
    var total = 0
    xs.foreach(x => total += x)
    total
  }

  def main(args: Array[String]): Unit = {
    val n: Any = upcast[Int, Any](42)
    println(n)
    val s: String = sameType[String, String]("hello")
    println(s)
    val some: Option[String] = Some("present")
    println(some.orNull)
    val none: Option[String] = None
    println(none.orNull)
    println(sumAll(List(1, 2, 3, 4)))
  }
}
