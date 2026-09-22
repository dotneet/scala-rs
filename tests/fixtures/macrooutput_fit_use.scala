object Main {
  val first: Result[Int] = implicitly[Result[Int]]
  val second: Result[Long] = implicitly[Result[Long]]
  def direct[A, R](input: A)(implicit evidence: Evidence[A] { type Out = R }, render: Render[R]): String =
    render(evidence.value)
  val third: String = direct(1.0)
  def main(args: Array[String]): Unit = {
    println(first.value)
    println(second.value)
    println(third)
  }
}
