// When the implicit is not found. Pins that the paths `itail.scala` opened up
// did not turn into "passes even when nothing is found".
//
// 1. A residual implicit clause in argument position is filled with the evidence
//    the parameter type asks for, never with the one implicit that happens to
//    be in scope.
// 2. A type parameter no value argument mentions is decided by implicit search,
//    but with no candidate at all it stays undecided.

class Tagged[T](val name: String)
object Tagged {
  implicit val intTag: Tagged[Int] = new Tagged[Int]("int")
}

class Sized[T](val n: Int)

object Bad {
  def take(xs: Sized[String]): Int = xs.n
  def empty[T](implicit t: Tagged[T]): Sized[T] = new Sized[T](0)

  // There is no `Tagged[String]`, so the residual implicit clause cannot be filled.
  val a: Int = take(empty)

  def rows[T](prefix: String)(implicit sz: Sized[T]): String = prefix + sz.n

  // There is no `Sized` implicit anywhere, so `T` cannot be decided.
  val b = rows("p")
}
