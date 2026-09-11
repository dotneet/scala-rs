object Main {
  def f: ((=> Int) => Int) = value => value
  val wrong = f(() => 7)
}
