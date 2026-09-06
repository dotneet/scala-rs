import scala.annotation.tailrec
final class TrcCounter(val value: Int) {
  @tailrec final def hop(n: Int, other: TrcCounter): Int =
    if (n == 0) value else other.hop(n - 1, this)
  @tailrec private def count(n: Int, total: Long): Long =
    if (n == 0) total else count(n - 1, total + n)
  def total(n: Int): Long = count(n, 0L)
}
object TrcDeep {
  @tailrec def wide(n: Int, a: Long, b: Long, d: Double): Long =
    if (n == 0) a - b + d.toLong else wide(n - 1, b, a, d + 1.0)
  @tailrec def matching(n: Int, acc: Int): Int = n match {
    case 0 => acc
    case 1 => matching(0, acc + 1)
    case _ => { val next = n - 1; matching(next, acc + 1) }
  }
  def local(n: Int): Int = {
    val step = 2
    @tailrec def loop(k: Int, acc: Int): Int =
      if (k == 0) acc else loop(k - 1, acc + step)
    loop(n, 0)
  }
  var trace = ""
  def arg(n: Int): Int = { trace = trace + n; n }
  @tailrec def order(n: Int, a: Int, b: Int): Int =
    if (n == 0) a * 10 + b else order(n - 1, arg(b), arg(a))
  def main(args: Array[String]): Unit = {
    println(wide(2000001, 10L, 20L, 0.0))
    println(matching(2000000, 0))
    println(local(2000000))
    val a = new TrcCounter(7)
    val b = new TrcCounter(9)
    println(a.hop(2000001, b))
    println(a.total(2000000))
    println(order(2, 1, 2))
    println(trace)
    println(TrcShapes.curried(2000000)(0L))
    println(TrcShapes.generic[String](2000001, "a", "b"))
    println(TrcShapes.unit(2000000, (), 0L))
    println(TrcShapes.byname(2000000, 42))
    println(TrcShapes.unannotated(2000000, 0))
    println(TrcShapes.mutableCapture(2000000))
    val end = new TrcLink(null)
    var link = end
    var k = 0
    while (k < 1000000) { link = new TrcLink(link); k += 1 }
    println(link.last == end)
    trace = ""
    println(new TrcNull().hop(2, null))
    println(trace)
    println(new TrcOverride().loop(2))
    println(TrcShapes.passLong(42))
    println(TrcShapes.passAny(43))
  }
}
object TrcShapes {
  def takeLong(x: => Long): Long = x
  def passLong(x: => Int): Long = takeLong(x)
  def takeAny(x: => Any): Any = x
  def passAny(x: => Int): Any = takeAny(x)
  @tailrec def curried(n: Int)(a: Long): Long = if (n == 0) a else curried(n - 1)(a + 1L)
  @tailrec def generic[A](n: Int, a: A, b: A): A = if (n == 0) a else generic[A](n - 1, b, a)
  @tailrec def unit(n: Int, u: Unit, a: Long): Long = if (n == 0) a else unit(n - 1, (), a + 1L)
  @tailrec def byname(n: Int, x: => Int): Int = if (n == 0) x else byname(n - 1, x)
  final def unannotated(n: Int, x: Int): Int = if (n == 0) x else unannotated(n - 1, x + 1)
  def mutableCapture(n: Int): Int = {
    var count = 0
    @tailrec def loop(k: Int): Int = if (k == 0) count else { count += 1; loop(k - 1) }
    loop(n)
  }
}
final class TrcLink(val next: TrcLink) {
  @tailrec final def last: TrcLink = if (next == null) this else next.last
}
final class TrcNull {
  @tailrec def hop(n: Int, other: TrcNull): Int =
    if (n == 0) 0 else other.hop(TrcDeep.arg(n - 1), this)
}
class TrcVirtual {
  def loop(n: Int): Int = if (n == 0) 1 else loop(n - 1)
}
class TrcOverride extends TrcVirtual {
  override def loop(n: Int): Int = if (n == 1) 99 else super.loop(n)
}
