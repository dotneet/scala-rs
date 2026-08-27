object Main {
  def main(args: Array[String]): Unit = {
    println("abc".foldRight("")((c, s) => s + c))
    println("12".toByteOption)
    println("nope".toByteOption)
    println("12".toShortOption)
    println("1.5".toFloatOption)
    "abcdef".grouped(2).foreach(s => println(s))
  }
}
