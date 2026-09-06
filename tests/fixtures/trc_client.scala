object TrcClient {
  def main(args: Array[String]): Unit = {
    println(new TrcCounter(7).total(2000000))
    println(TrcDeep.wide(2000001, 10L, 20L, 0.0))
  }
}
