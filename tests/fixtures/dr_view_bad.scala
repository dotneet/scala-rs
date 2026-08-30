// 異常系：適合する implicit view が無いときは今までどおり拒否されること。
// 実 scalac も "No implicit view available from Plain => Ordered[Plain]" で
// 落ちる。受け入れすぎていないかの歯止め。
class Plain(val n: Int)

object Main {
  def h[A](x: A)(implicit ev: A => Ordered[A]): A = x

  def main(args: Array[String]): Unit = {
    println(h(new Plain(1)))
    println(h(new Object))
  }
}
