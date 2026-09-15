object Main {
  def main(args: Array[String]): Unit = {
    println(RefinedBundle.identity[Int](42))
    println(RefinedBundle.identity[String]("bundle"))
    println(RefinedBundle.answer)
  }
}
