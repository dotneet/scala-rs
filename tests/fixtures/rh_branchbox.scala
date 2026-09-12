// An `if` / `match` whose arms are different *primitives* and whose own type is a
// reference. The arms meet at one stack slot, so each one has to box -- even when
// nothing outside the expression asked for a type, which is exactly what
// `{ … }.asInstanceOf[A]` does: the receiver of a cast is typed on its own. A
// `long` arm then reached a one-slot frame: "VerifyError: Inconsistent stackmap
// frames ... stack: { long, long_2nd } / Stackmap Frame ... stack: { top }".
//
// `convert` is gitbucket's `ConfigUtil.convertType[A: ClassTag]`, which
// `DatabaseConfig` reads a value through before gitbucket does anything at all --
// so every gitbucket program died on it.
object Main {
  def convert[A](value: String, want: Int): A = {
    if (want == 0) value.toBoolean
    else if (want == 1) value.toLong
    else if (want == 2) value.toInt
    else value
  }.asInstanceOf[A]

  def viaMatch[A](value: String, want: Int): A = {
    want match {
      case 0 => value.toDouble
      case 1 => value.toFloat
      case 2 => value.head
      case _ => value
    }
  }.asInstanceOf[A]

  def nested[A](b: Boolean): A = {
    if (b) { if (b) 3L else 4.0f } else "x"
  }.asInstanceOf[A]

  // The same join with the expected type present, which always worked.
  def plain(b: Boolean): Any = if (b) 1L else "x"

  def main(args: Array[String]): Unit = {
    println(convert[Boolean]("true", 0))
    println(convert[Long]("123", 1))
    println(convert[Int]("45", 2))
    println(convert[String]("hello", 3))
    println(viaMatch[Double]("1.5", 0))
    println(viaMatch[Float]("2.5", 1))
    println(viaMatch[Char]("zed", 2))
    println(viaMatch[String]("zed", 3))
    println(nested[Any](true))
    println(nested[Any](false))
    println(List(true, false).map(plain))
  }
}
