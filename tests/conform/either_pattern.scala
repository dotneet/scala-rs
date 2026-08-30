// `case Right(v)` / `case Left(s)` read `value` through the accessor.
//
// `Left.value` and `Right.value` are private fields in the library, so a
// pattern that binds them has to go through the nullary accessor. Declaring
// only the constructor field made the pattern emit a `getfield` and throw
// `IllegalAccessError` at run time -- it type-checked cleanly. `Success` and
// `Failure` already carried the accessor for exactly this reason.
object Main {
  def divide(a: Int, b: Int): Either[String, Int] =
    if (b == 0) Left("div by zero") else Right(a / b)

  def main(args: Array[String]): Unit = {
    val rs = List((10, 2), (1, 0), (9, 3)).map { case (a, b) => divide(a, b) }
    println(rs)
    println(rs.collect { case Right(v) => v }.sum)
    println(rs.collect { case Left(e) => e })
    val e: Either[String, Int] = Right(3)
    e match {
      case Right(v) => println("r" + v)
      case Left(s)  => println("l" + s)
    }
    val f: Either[String, Int] = Left("bad")
    f match {
      case Right(v) => println("r" + v)
      case Left(s)  => println("l" + s)
    }
    println(rs.flatMap(_.toOption))
  }
}
