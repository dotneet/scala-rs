object Main {
  def main(args: Array[String]): Unit = {
    println(1L to 3L)
    println(1L until 3L)
    println((1L to 3L).mkString(","))
    println((1L until 3L).mkString(","))
  }
}
