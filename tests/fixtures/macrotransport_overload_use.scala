object Main {
  def main(args: Array[String]): Unit = {
    println(ChosenMacro.inspect("a", new Exception("test")))
    println(ChosenMacro.inspect("b", 42))
  }
}
