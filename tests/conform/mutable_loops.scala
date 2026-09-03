// Mutable state: var reassignment inside while/do-while conditions, arrays,
// nested loops with early exit, and a mutable builder.
object Main {
  def gcd(a0: Int, b0: Int): Int = {
    var a = a0; var b = b0
    while (b != 0) { val t = b; b = a % b; a = t }
    if (a < 0) -a else a
  }

  def firstMissing(xs: Array[Int]): Int = {
    var i = 0
    var found = true
    var n = 1
    while (found) {
      found = false
      i = 0
      while (i < xs.length) { if (xs(i) == n) found = true; i += 1 }
      if (found) n += 1
    }
    n
  }

  def matmul(a: Array[Array[Int]], b: Array[Array[Int]]): Array[Array[Int]] = {
    val n = a.length; val m = b(0).length; val k = b.length
    val out = Array.ofDim[Int](n, m)
    var i = 0
    while (i < n) {
      var j = 0
      while (j < m) {
        var s = 0; var t = 0
        while (t < k) { s += a(i)(t) * b(t)(j); t += 1 }
        out(i)(j) = s; j += 1
      }
      i += 1
    }
    out
  }

  def main(args: Array[String]): Unit = {
    println(gcd(48, 18)); println(gcd(-48, 18)); println(gcd(0, 5))
    println(firstMissing(Array(1, 2, 4, 5)))

    val a = Array(Array(1, 2), Array(3, 4))
    val b = Array(Array(5, 6), Array(7, 8))
    println(matmul(a, b).map(_.mkString(",")).mkString(";"))

    var i = 0
    var acc = List.empty[Int]
    do { acc ::= i; i += 1 } while (i < 3)
    println(acc)

    var s = 0
    for (x <- 1 to 5; if x % 2 == 1) s += x
    println(s)
    for (x <- 1 to 3; y <- 1 to 3 if x < y) print(s"$x$y ")
    println()

    val buf = scala.collection.mutable.ListBuffer.empty[String]
    var n = 5
    while ({ n -= 1; n > 0 }) buf += n.toString
    println(buf.toList)

    val arr = Array.fill(5)(0)
    var j = arr.length - 1
    while (j >= 0) { arr(j) = j * j; j -= 1 }
    println(arr.mkString("[", " ", "]"))
    println(arr.sum)

    var flag = false
    var count = 0
    while (!flag && count < 10) { count += 1; if (count == 4) flag = true }
    println(count + " " + flag)

    val m = scala.collection.mutable.Map.empty[Char, Int]
    "abracadabra".foreach(c => m(c) = m.getOrElse(c, 0) + 1)
    println(m.toList.sorted)

    var opt: Option[Int] = None
    var k2 = 0
    while (opt.isEmpty) { k2 += 1; if (k2 > 3) opt = Some(k2) }
    println(opt)

    var total = 0
    val it = List(1, 2, 3).iterator
    while (it.hasNext) total += it.next()
    println(total)
  }
}
