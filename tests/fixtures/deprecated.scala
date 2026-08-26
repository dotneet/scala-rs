object Main {
  @deprecated("old")
  def f(): Int = 41
  def main(args: Array[String]): Unit = {
    println(f() + 1)
  }
}
