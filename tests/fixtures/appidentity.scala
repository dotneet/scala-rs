package custom {
  trait App { def value: Int }
  trait AppBase extends App
  class AppChild extends AppBase {
    println("custom-app")
    val value: Int = 7
  }
  trait DelayedInit
  class DelayedChild extends DelayedInit {
    println("custom-delayed")
    val value: Int = 9
  }
}
object RealApplication extends scala.App { println("real-app") }
class RealDelayed extends scala.DelayedInit {
  def delayedInit(body: => Unit): Unit = { println("real-delayed-hook"); body }
  println("real-delayed-body")
}
object Main {
  def main(args: Array[String]): Unit = {
    println(new custom.AppChild().value)
    println(new custom.DelayedChild().value)
    val a = new custom.App { def value: Int = 11 }
    println(a.value)
    RealApplication.main(new Array[String](0))
    new RealDelayed
  }
}
