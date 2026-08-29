object Main {
  def main(args: Array[String]): Unit = {
    val a = 8L
    println(a >> 2); println(a << 2); println((-8L) >>> 2)
    println(a & 12L); println(a | 3L); println(a ^ 12L); println(~a)
    println(a >> 2L)
    println(8 >> 2); println(8 & 12); println(~8)
    println(a & 12); println(8 & 12L)
  }
}
