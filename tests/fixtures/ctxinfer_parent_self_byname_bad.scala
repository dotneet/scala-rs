class LazyBase(value: => AnyRef) {
  def current: AnyRef = value
}
object Main extends LazyBase(Main) {
  def main(args: Array[String]): Unit = println(current eq Main)
}
