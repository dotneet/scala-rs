object BinaryByNameApi {
  def evaluate[A](value: => A): A = value
}
