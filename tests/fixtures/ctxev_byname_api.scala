object ByNameApi {
  def twice: ((=> Int) => Int) = value => value + value
  def repeat(value: => Int): Int = value + value
  def hold[A](value: => A): A = value
}
