object Main {
  @inline val x: Int = 1
  @inline @noinline def f(): Int = 1
  def main(args: Array[String]): Unit = {
    println(x)
  }
}
