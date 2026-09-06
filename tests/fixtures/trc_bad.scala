import scala.annotation.tailrec
class TrcOverridable {
  @tailrec def loop(n: Int): Int = if (n == 0) 0 else loop(n - 1)
}
object TrcNonTail {
  @tailrec def loop(n: Int): Int = if (n == 0) 0 else 1 + loop(n - 1)
}
