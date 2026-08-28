object Main {
  def main(args: Array[String]): Unit = {
    val m = Map("a" -> 1)
    val x = m.view.noSuchMapValues
  }
}
