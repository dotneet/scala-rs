class C extends DelayedInit {
  def delayedInit(x: => Unit): Unit = x
  println("from-ctor")
}
object Main {
  def main(args: Array[String]): Unit = {
    new C
  }
}
