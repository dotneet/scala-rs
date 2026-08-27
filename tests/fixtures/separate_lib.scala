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
  def nest(xs: List[_ <: List[_]]): Int = 0
  def idRef(x: MixA with MixB { def f: Int }): MixA with MixB { def f: Int } = x
}
trait MixA { def a: Int }
trait MixB { def b: Int }
class MixD extends MixA with MixB {
  def a: Int = 1
  def b: Int = 2
  def f: Int = 3
}
class Box[A](val value: A) {
  def get: A = value
}
