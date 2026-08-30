object Main {
  def sumFromOne[T](n: Int)(implicit integral: Integral[T]): T = {
    import integral._
    var acc = zero
    var i = one
    var k = n
    while (k > 0) {
      acc = plus(acc, i)
      i = plus(i, one)
      k -= 1
    }
    plus(acc, fromInt(0))
  }

  def main(args: Array[String]): Unit = {
    println(sumFromOne[Int](3))
    println(sumFromOne[Long](4))
  }
}
