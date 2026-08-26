object Lib {
  val magic: Int = 7
  def greet(name: String, punct: String = "!"): String = "hi " + name + punct
  def id[T](x: T): T = x
  def add(p: Point): Int = p.x + p.y
}
class Box[A](val value: A) {
  def get: A = value
}
case class Point(x: Int, y: Int)

