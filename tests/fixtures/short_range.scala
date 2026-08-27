object Main {
  def main(args: Array[String]): Unit = {
    println(1.toShort to 3.toShort)
    println(1.toShort until 3.toShort)
    println((1.toShort to 3.toShort).mkString(","))
    println((1.toShort until 3.toShort).mkString(","))
  }
}
