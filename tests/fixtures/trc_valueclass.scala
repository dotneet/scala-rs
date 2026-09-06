import scala.annotation.tailrec

final class TrcInt(val value: Int) extends AnyVal {
  @tailrec final def loop(n: Int, acc: Int): Int =
    if (n == 0) acc + value else loop(n - 1, acc + 1)

  @tailrec final def receiverLoop(n: Int): Int =
    if (n == 0) value else new TrcInt(value + 1).receiverLoop(n - 1)
}

final class TrcLong(val value: Long) extends AnyVal {
  @tailrec final def loop(n: Int, acc: Long): Long =
    if (n == 0) acc + value else loop(n - 1, acc + 1L)

  @tailrec final def receiverLoop(n: Int): Long =
    if (n == 0) value else new TrcLong(value + 1L).receiverLoop(n - 1)

  @tailrec final def receiverAndArgs(n: Int, acc: Long): Long =
    if (n == 0) acc + value
    else new TrcLong(value + 1L).receiverAndArgs(n - 1, acc + value)

  @tailrec final def effects(n: Int, acc: Long): Long =
    if (n == 0) acc + value
    else TrcValueclassState.receiver().effects(n - 1, TrcValueclassState.argument())
}

final class TrcDouble(val value: Double) extends AnyVal {
  @tailrec final def loop(n: Int, acc: Double): Double =
    if (n == 0) acc + value else loop(n - 1, acc + 1.0)

  @tailrec final def receiverLoop(n: Int): Double =
    if (n == 0) value else new TrcDouble(value + 1.0).receiverLoop(n - 1)
}

final class TrcRef(val value: String) extends AnyVal {
  @tailrec final def loop(n: Int, acc: String): Int =
    if (n == 0) acc.length + value.length else loop(n - 1, acc)

  @tailrec final def receiverLoop(n: Int): Int =
    if (n == 0) value.length else new TrcRef(value).receiverLoop(n - 1)
}

object TrcValueclassState {
  var events: Int = 0

  def receiver(): TrcLong = {
    events = events * 10 + 1
    new TrcLong(0L)
  }

  def argument(): Long = {
    events = events * 10 + 2
    0L
  }
}

object TrcValueclass {
  def main(args: Array[String]): Unit = {
    println(new TrcInt(7).loop(2000000, 0))
    println(new TrcLong(7L).loop(2000000, 0L))
    println(new TrcDouble(7.0).loop(2000000, 0.0))
    println(new TrcRef("!").loop(2000000, ""))
    println(new TrcInt(7).receiverLoop(2000000))
    println(new TrcLong(7L).receiverLoop(2000000))
    println(new TrcLong(7L).receiverAndArgs(2000000, 0L))
    println(new TrcDouble(7.0).receiverLoop(2000000))
    println(new TrcRef("!").receiverLoop(2000000))
    println(new TrcLong(0L).effects(2, 0L))
    println(TrcValueclassState.events)
  }
}
