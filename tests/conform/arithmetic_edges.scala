object Main {
  def main(a: Array[String]): Unit = {
    println(1 + 2 * 3 - 4 / 2)
    println(1 :: 2 :: Nil)
    println(true && false || true)
    println(5 % 3, -5 % 3, 5 / -3)
    println(Int.MaxValue + 1)
    println(1 << 31, -1 >>> 28, -1 >> 28)
    println(0.1 + 0.2)
    println(1.0 / 0, -1.0 / 0, 0.0 / 0.0)
    println(Long.MinValue, Int.MinValue.abs)
    println('a' + 1, 'a'.toInt, ('a' + 1).toChar)
    println(3.toByte, 300.toByte, (-1).toChar.toInt)
  }
}
