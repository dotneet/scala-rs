class X(val value: Int) {
  def f(n: Any): Int = -1
  object f { def apply(n: Int): Int = value + n }
}
object Main extends App {
  var calls = 0
  def receiver: X = { calls += 1; new X(40) }
  println(receiver.f(2))
  println(calls)
}
