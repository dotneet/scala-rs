// The rejection side: with no conforming implicit view, this is still refused.
// Real scalac fails with "No implicit view available from Plain => Ordered[Plain]"
// too. A stop against accepting too much.
class Plain(val n: Int)

object Main {
  def h[A](x: A)(implicit ev: A => Ordered[A]): A = x

  def main(args: Array[String]): Unit = {
    println(h(new Plain(1)))
    println(h(new Object))
  }
}
