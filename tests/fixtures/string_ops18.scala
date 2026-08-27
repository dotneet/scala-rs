object Main {
  def main(args: Array[String]): Unit = {
    println("abc".foldLeft("")((s, c) => s + c))
    println("12".toByte)
    println("12".toShort)
    println("1.5".toFloat)
    println("9".toLongOption)
    println("nope".toLongOption)
    println("1.5".toDoubleOption)
  }
}
