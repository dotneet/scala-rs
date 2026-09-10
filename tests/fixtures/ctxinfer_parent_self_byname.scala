class LazyBase(value: => AnyRef) {
  def current: AnyRef = value
}
object Other
object State { var calls: Int = 0; def next: AnyRef = { calls += 1; Other } }
object Main extends LazyBase(State.next) {
  def main(args: Array[String]): Unit = {
    println(State.calls)
    println(current eq Other)
    println(current eq Other)
    println(State.calls)
  }
}
