object Main {
  def accept(s: ValueCodec[String]): String = s.name
  def main(args: Array[String]): Unit = {
    val value: ValueCodec[String] = ReceiverMacro.wrapped.value
    println(value.name)
    println(accept(ReceiverMacro.wrapped.value))
  }
}
