class NoOrd(val n: Int)
class C[A <% Ordered[A]](x: A)
object Main {
  def main(args: Array[String]): Unit = {
    val c = new C(new NoOrd(1))
  }
}
