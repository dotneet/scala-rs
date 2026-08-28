object Main {
  def d(x: Double): Double = x * 2
  def l(x: Long): Long = x + 1
  def f(x: Float): Float = x
  def main(args: Array[String]): Unit = {
    println(d(3))
    println(l(2))
    println(f(3))
    val y: Double = 5
    println(y)
    val z: Long = 7
    println(z)
    println(1 + 2.5)
    println(2L + 1)
    println(1.5 * 2)
    println(d(2L))
  }
}
