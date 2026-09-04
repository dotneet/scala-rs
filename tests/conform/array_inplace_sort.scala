// Sorting in place (update / swap / while), and the Array factories.
object Main {
  def swap(a: Array[Int], i: Int, j: Int): Unit = {
    val t = a(i)
    a(i) = a(j)
    a(j) = t
  }

  def bubble(a: Array[Int]): Array[Int] = {
    var n = a.length
    while (n > 1) {
      var i = 0
      while (i < n - 1) {
        if (a(i) > a(i + 1)) swap(a, i, i + 1)
        i += 1
      }
      n -= 1
    }
    a
  }

  def main(args: Array[String]): Unit = {
    println(bubble(Array(5, 3, 9, 1, 4)).mkString(","))
    println(Array.fill(4)(7).mkString(""))
    println(Array.tabulate(5)(i => i * i).mkString("+"))
    println(Array.fill(3)("x").mkString("-"))
    println(Array.ofDim[Int](3).mkString("."))
    println(Array.empty[Int].length)

    val buf = new Array[String](3)
    buf(0) = "a"
    buf(2) = "c"
    println(buf.map(s => if (s == null) "?" else s).mkString(""))

    val copy = bubble(Array(2, 1)).clone()
    copy(0) = 8
    println(copy.mkString("/"))
  }
}
