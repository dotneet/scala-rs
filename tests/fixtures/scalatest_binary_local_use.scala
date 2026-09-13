object Main {
  def main(args: Array[String]): Unit = {
    val value = regression.BinaryValue(7)
    println(org.scalatest.BinaryAssertions.retypeSelected(value.member))
  }
}
