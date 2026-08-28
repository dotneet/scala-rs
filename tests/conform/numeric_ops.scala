object Main {
  def main(a: Array[String]): Unit = {
    val pi = 3.14159
    println(pi > 3.0); println(pi < 3.0); println(pi >= 3.14159); println(pi <= 3.0)
    println(pi == 3.14159); println(pi != 3.0)
    println(pi % 2.0)
    val f = 1.5f
    println(f > 1.0f); println(f + 0.5f); println(f * 2.0f); println(f % 1.0f)
    println(5L % 3L); println(5L > 3L); println(5L <= 3L); println(5L == 5L)
    val nan = Double.NaN
    println(nan < 1.0); println(nan > 1.0); println(nan == nan)
    println(pi > 3); println(3 < pi)
  }
}
