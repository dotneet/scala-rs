// while and do-while, Breaks, @tailrec methods (including in a final class
// and with multiple accumulators), deep recursion that must be a loop.
import scala.annotation.tailrec
import scala.util.control.Breaks._
object Main {
  @tailrec def gcd(a: Long, b: Long): Long = if (b == 0) a else gcd(b, a % b)
  @tailrec def count(n: Int, acc: Int = 0): Int = if (n == 0) acc else count(n - 1, acc + 1)
  def fact(n: Int): BigInt = { @tailrec def go(n: Int, acc: BigInt): BigInt = if (n <= 1) acc else go(n - 1, acc * n); go(n, 1) }
  @tailrec def find[A](xs: List[A], p: A => Boolean): Option[A] = xs match { case Nil => None; case h :: t => if (p(h)) Some(h) else find(t, p) }
  final class Walker(val step: Int) { @tailrec def walk(pos: Int, n: Int): Int = if (n == 0) pos else walk(pos + step, n - 1) }
  @tailrec def fib(n: Int, a: Long = 0, b: Long = 1): Long = if (n == 0) a else fib(n - 1, b, a + b)
  @tailrec def sumD(xs: List[Double], acc: Double): Double = xs match { case Nil => acc; case h :: t => sumD(t, acc + h) }
  @tailrec def isEven(n: Int, flag: Boolean = true): Boolean = if (n == 0) flag else isEven(n - 1, !flag)
  def nonTail(n: Int): Int = if (n == 0) 0 else 1 + nonTail(n - 1)

  def main(args: Array[String]): Unit = {
    println(gcd(1071, 462) + " " + count(1000000) + " " + fact(25) + " " + find(List(1, 4, 9), (x: Int) => x > 3))
    println(new Walker(3).walk(0, 100000) + " " + fib(90) + " " + sumD(List(0.5, 0.25, 0.125), 0) + " " + isEven(100001))
    println(nonTail(1000))
    var i = 0; var s = 0
    while (i < 5) { s += i; i += 1 }
    println(s"$i $s")
    var j = 10
    do { j -= 3 } while (j > 0)
    println(j)
    var k = 0
    do k += 1 while (k < 0)
    println(k)
    var out = List.empty[Int]
    breakable { for (x <- 1 to 10) { if (x == 4) break(); out ::= x } }
    println(out.reverse)
    var outer = 0
    for (a <- 1 to 3) breakable { for (b <- 1 to 3) { if (b == 2) break(); outer += a * 10 + b } }
    println(outer)
    var w = 0; var guard = 0
    while ({ guard += 1; guard < 4 }) w += guard
    println(w + " " + guard)
    val it = Iterator.iterate(1)(_ * 2).takeWhile(_ < 100)
    println(it.toList)
    var n = 27; var steps = 0
    while (n != 1) { n = if (n % 2 == 0) n / 2 else 3 * n + 1; steps += 1 }
    println(steps)
    var lp = 0L
    while (lp < 3000000000L) lp += 1000000000L
    println(lp)
  }
}
