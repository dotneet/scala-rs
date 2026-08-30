object Main {
  def quicksort(xs: List[Int]): List[Int] = xs match {
    case Nil => Nil
    case p :: rest =>
      val (lo, hi) = rest.partition(_ < p)
      quicksort(lo) ::: p :: quicksort(hi)
  }
  def primes(n: Int): List[Int] = {
    val sieve = Array.fill(n + 1)(true)
    for (i <- 2 to n; if sieve(i); j <- i * i to n by i) sieve(j) = false
    (2 to n).filter(sieve).toList
  }
  def main(a: Array[String]): Unit = {
    println(quicksort(List(5,3,8,1,9,2,7)))
    println(primes(30))
    val fib = Iterator.iterate((0L, 1L)) { case (a, b) => (b, a + b) }.map(_._1)
    println(fib.take(12).toList)
  }
}
