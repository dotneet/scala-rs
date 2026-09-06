import scala.annotation.tailrec
class TrcValue(val value: Int) extends AnyVal {
  @tailrec final def loop(n: Int): Int = if (n == 0) value else loop(n - 1)
}
