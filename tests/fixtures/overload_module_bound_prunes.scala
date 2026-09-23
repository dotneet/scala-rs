trait Base
final class Child extends Base

object Factory {
  def apply[T <: Base](creator: => T): String = "generic"
  def apply(clazz: Class[_], args: Any*): String = "class"
}

object Main extends App {
  println(Factory(classOf[Child]))
}
