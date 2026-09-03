// 小さな行列演算（Array[Array[Double]] と Array[Array[Int]]）。
object Main {
  type Mat = Array[Array[Double]]

  def zeros(r: Int, c: Int): Mat = Array.ofDim[Double](r, c)

  def mul(a: Mat, b: Mat): Mat = {
    val out = zeros(a.length, b(0).length)
    for (i <- a.indices; j <- b(0).indices) {
      var s = 0.0
      for (k <- b.indices) s += a(i)(k) * b(k)(j)
      out(i)(j) = s
    }
    out
  }

  def show(m: Mat): String = m.map(_.mkString(",")).mkString(";")

  def main(args: Array[String]): Unit = {
    val a: Mat = Array(Array(1.0, 2.0), Array(3.0, 4.0))
    val b: Mat = Array(Array(0.0, 1.0), Array(1.0, 0.0))
    println(show(mul(a, b)))
    println(show(zeros(2, 3)))

    val grid = Array(Array(1, 2, 3), Array(4, 5, 6))
    println(grid.map(_.sum).mkString("/"))
    println(grid.map(_.mkString("")).mkString("|"))
    println(grid.flatMap(r => r.map(_ * 2)).mkString(","))
    println(grid(1)(2))
    grid(0)(0) = 9
    println(grid.map(_.head).mkString("."))
  }
}
