case class Point(x: Int, y: Int)
object Lib {
  val magic: Int = 7
  def greet(name: String, punct: String = "!"): String = "hi " + name + punct
  def id[T](x: T): T = x
  def add(p: Point): Int = p.x + p.y
  final def f(xs: List[_]): Int = 0
  @deprecated("msg", "2.13.0") def g: Int = 2
}
class Box[A](val value: A) {
  def get: A = value
}

