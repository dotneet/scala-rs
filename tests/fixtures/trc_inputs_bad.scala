import scala.annotation.tailrec
final class TrcBadReceiver {
  @tailrec def loop(n: Int): TrcBadReceiver =
    if (n == 0) this else loop(0).loop(n - 1)
}
object TrcBadCurried {
  @tailrec def loop(n: Int)(m: Int): Int =
    if (n == 0) m else loop(loop(n - 1)(m))(m)
}
