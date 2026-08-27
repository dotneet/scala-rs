object Main {
  def main(args: Array[String]): Unit = {
    println(1 to 3)
    println(1 until 3)
    (1 to 3).foreach(x => println(x))
    println((1 to 3).mkString(","))
    println((1 until 3).mkString(","))
    println(1.toByte to 3.toByte)
    println(1.toByte until 3.toByte)
    println((1.toByte to 3.toByte).mkString(","))
    println((1.toByte until 3.toByte).mkString(","))
  }
}
