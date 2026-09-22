object Main {
  case class Entry(value: Int) {
    def visible: Int = value
    private def hidden: Int = value
  }
  def main(args: Array[String]): Unit = {
    println(ReflectionHelpers.inspect[Entry])
    println(ReflectionHelpers.inspect[BinaryEntry])
    println(SymbolicReflection.value)
  }
}
