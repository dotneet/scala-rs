object Main {
  // Dropping unreachable code from the bytecode must not stop it from being
  // typechecked: the trailing String is still an error against the Int result.
  def f(): Int = {
    throw new RuntimeException("x")
    "not an int"
  }
  def main(args: Array[String]): Unit = println(f())
}
