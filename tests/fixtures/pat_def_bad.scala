object Main {
  def f: Int = {
    val Some(v) = Some(9)
    v.nosuchmember
  }
}
