object Main {
  def take(value: => Int): Int = value
  val wrong = take(() => 7)
}
