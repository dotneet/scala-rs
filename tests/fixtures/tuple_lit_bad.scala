object Main {
  def f: Int = {
    val (a, b) = (1, "x")
    b.nosuchmember
  }
}
