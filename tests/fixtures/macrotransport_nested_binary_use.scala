object Main {
  def main(args: Array[String]): Unit = {
    val value: BinaryOuter#Inner = BinaryCast.cast[BinaryOuter#Inner](null)
    println(value == null)
  }
}
