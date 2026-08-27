case class Point(x: Int, y: Int)
class Ann(x: Any) extends annotation.StaticAnnotation
class Base { val foo = 1 }
class Holder extends Base {
  def me: this.type = this
  def n: Int = 1
  val x = 1
  @Ann(this) def markedThis: Int = 4
  @Ann(classOf[Int]) def markedClass: Int = 5
  @Ann(this.x) def markedThisSel: Int = 7
  @Ann(super.foo) def markedSuper: Int = 8
}
class C { val x = 1 }
class OrdBox(val n: Int) extends Ordered[OrdBox] {
  def compare(that: OrdBox): Int = n - that.n
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
  val foo = 1
  val c = new C
  @Ann(foo) def marked: Int = 1
  @Ann(c.x) def markedSel: Int = 2
  @Ann(3) def markedLit: Int = 3
  def ident(n: Int): Int = n
  @Ann(ident(1)) def markedApply: Int = 6
  @Ann(ident(ident(1))) def markedNest: Int = 9
  @Ann(foo = 1) def markedNamed: Int = 10
  def join(xs: String*): Int = 0
}
trait MixA { def a: Int }
trait MixB { def b: Int }
class Box[A](val value: A) {
  def get: A = value
}
