import scala.language.dynamics
class Dispatch extends Dynamic {
  def applyDynamic[A](name: String)(x: A): String = name + ":1"
  def applyDynamic[A, B](name: String)(x: A, y: B): String = name + ":2"
  def applyDynamic[A, B, C](name: String)(x: A, y: B, z: C): String = name + ":3"
  def real[A](x: A): String = "real"
}
class RealApply extends Dynamic {
  def apply[A](x: A): String = "ordinary"
  def applyDynamic[A](name: String)(x: A): String = "wrong"
}
object Main {
  def as[A](implicit value: String): String = value
  implicit val default: String = "wrong"
  var receivers = 0
  def receiver(): Dispatch = { receivers += 1; new Dispatch }
  def main(args: Array[String]): Unit = {
    val x = new Dispatch
    println(x[Int](1))
    println(x[Int, String](1, "a"))
    println(x[Int, String, Int](1, "a", 2))
    println(x.foo[Int, String](1, "a"))
    println(x.real[Int](1))
    println(new RealApply().apply[Int](1))
    println(receiver().bar[Int](1))
    println(receivers)
    println(as[Int]("explicit"))
  }
}
