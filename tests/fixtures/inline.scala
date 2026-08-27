object Main {
  @inline def f(): Int = 1
  @noinline def g(): Int = 2
  def main(args: Array[String]): Unit = {
    println(f() + g())
  }
}
