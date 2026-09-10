object Main {
  def main(args: Array[String]): Unit = {
    println(InferredMacro.identity {
      def value = 40
      InferredMacro.identity(value): @unchecked
      value + 2
    })
  }
}
