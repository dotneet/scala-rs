import scala.annotation.tailrec

final class TrcInt(val value: Int) extends AnyVal {
  @tailrec final def loop(n: Int, acc: Int): Int =
    if (n == 0) acc + value else loop(n - 1, acc + 1)
}

final class TrcLong(val value: Long) extends AnyVal {
  @tailrec final def loop(n: Int, acc: Long): Long =
    if (n == 0) acc + value else loop(n - 1, acc + 1L)
}

final class TrcDouble(val value: Double) extends AnyVal {
  @tailrec final def loop(n: Int, acc: Double): Double =
    if (n == 0) acc + value else loop(n - 1, acc + 1.0)
}

final class TrcRef(val value: String) extends AnyVal {
  @tailrec final def loop(n: Int, acc: String): Int =
    if (n == 0) acc.length + value.length else loop(n - 1, acc)
}

object TrcValueclass {
  def main(args: Array[String]): Unit = {
    println(new TrcInt(7).loop(2000000, 0))
    println(new TrcLong(7L).loop(2000000, 0L))
    println(new TrcDouble(7.0).loop(2000000, 0.0))
    println(new TrcRef("!").loop(2000000, ""))
  }
}
