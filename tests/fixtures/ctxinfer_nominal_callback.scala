object Main {
  def main(args: Array[String]): Unit = {
    val inferred = Callbacks.map(s => s.length)
    val checked: Seq[Int] = inferred
    val combined = Callbacks.combine((left, right) => left.length + right.length)
    val count: Int = combined
    println(checked.head)
    println(count)
  }
}
