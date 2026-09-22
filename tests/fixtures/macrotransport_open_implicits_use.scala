object Main {
  def main(args: Array[String]): Unit = {
    val info = implicitly[EvidenceInfo[String]]
    println(info.depth)
    println(info.expected)
  }
}
