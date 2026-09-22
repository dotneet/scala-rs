object Main {
  def main(args: Array[String]): Unit = {
    println(MacroContexts.outer)
    println(MacroContexts.inner)
  }
}
