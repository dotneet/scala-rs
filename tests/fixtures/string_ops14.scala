object Main {
  def main(args: Array[String]): Unit = {
    println("cba".sorted)
    "ab".toArray.foreach(c => println(c))
    val buf = new Array[Char](2)
    val n = "xy".copyToArray(buf)
    println(n)
    println(buf.head)
    println(buf.tail.head)
  }
}
