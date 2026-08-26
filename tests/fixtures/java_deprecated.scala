object Main {
  @Deprecated
  def old(): Int = 41
  def main(args: Array[String]): Unit = {
    println(old() + 1)
  }
}
