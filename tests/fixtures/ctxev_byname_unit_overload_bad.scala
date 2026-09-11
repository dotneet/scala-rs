object Main {
  def choose(value: => Unit): String = "unit"
  def choose(value: String): String = "string"
  val wrong = choose(() => ())
}
