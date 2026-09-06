object TrcValueclassClient {
  def main(args: Array[String]): Unit = {
    println(new TrcInt(7).loop(2000000, 0))
    println(new TrcLong(7L).loop(2000000, 0L))
    println(new TrcDouble(7.0).loop(2000000, 0.0))
    println(new TrcRef("!").loop(2000000, ""))
  }
}
