object Main {
  def take(value: => () => String): String = value()
  val wrong = take(() => 7)
}
