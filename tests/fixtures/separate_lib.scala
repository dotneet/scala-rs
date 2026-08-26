object Lib {
  val magic: Int = 7
  def greet(name: String, punct: String = "!"): String = "hi " + name + punct
  def id[T](x: T): T = x
}
class Box[A](val value: A) {
  def get: A = value
}
