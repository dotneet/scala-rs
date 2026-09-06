object OracleClassClient {
  def main(args: Array[String]): Unit = {
    val int: OracleBox[Int] = new OracleBox[Int](1)
    int.set(int.get + 2)
    val long: OracleBox[Long] = new OracleBox[Long](2L)
    long.set(long.get + 3L)
    val string: OracleBox[String] = new OracleBox[String]("s")
    string.set(string.get + "u")

    val childInt: OracleBox[Int] = new OracleIntBox
    childInt.set(childInt.get + 4)
    val childLong: OracleBox[Long] = new OracleLongBox
    childLong.set(childLong.get + 5L)
    val childString: OracleBox[String] = new OracleStringBox
    childString.set(childString.get + "v")

    val readable: OracleReadable[Int] = new OracleReadableInt
    println(
      s"${int.get}:${long.get}:${string.get}:${childInt.get}:${childLong.get}:${childString.get}:${readable.read}:${int.fallback("f")}"
    )
  }
}
