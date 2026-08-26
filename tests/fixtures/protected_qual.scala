class C {
  protected[C] val n: Int = 40
  def get: Int = n
  def fromPeer(other: C): Int = other.n
}
class D extends C {
  def mine: Int = this.n
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new C().get)
    println(new C().fromPeer(new C()))
    println(new D().mine)
  }
}
