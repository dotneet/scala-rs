case class Point(x: Int, y: Int)
class Holder {
  def me: this.type = this
  def n: Int = 1
}
object Lib {
  val magic: Int = 7
  def greet(name: String, punct: String = "!"): String = "hi " + name + punct
  def id[T](x: T): T = x
  def add(p: Point): Int = p.x + p.y
  final def f(xs: List[_]): Int = 0
  @deprecated("msg", "2.13.0") def g: Int = 2
  @Deprecated def gone: Int = 3
  def fAnyRef(xs: List[_ <: AnyRef]): Int = 0
  def h(x: Int @unchecked): Int = x
  val one: 1 = 1
  def lit(x: 1): Int = x
}
class Box[A](val value: A) {
  def get: A = value
}
