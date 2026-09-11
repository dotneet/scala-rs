class AliasOwner { type Event = String }
object Main {
  def main(args: Array[String]): Unit = {
    val o = new AliasOwner
    import o._
    def f(x: Event): String = x
    println(f(1))
  }
}
