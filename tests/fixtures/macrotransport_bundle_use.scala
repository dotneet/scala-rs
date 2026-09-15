object Main {
  def read[T](implicit evidence: LiteralEvidence[T]): T = evidence.value
  def main(args: Array[String]): Unit = {
    println(read[100])
    println(read["ready"])
  }
}
