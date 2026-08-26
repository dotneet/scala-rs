class C {
  def me: this.type = this
  def n: Int = 1
}
object Main {
  val c = new C()
  def id: c.type = c
  def main(args: Array[String]): Unit = {
    println(c.me.n)
    println(id.n)
  }
}
