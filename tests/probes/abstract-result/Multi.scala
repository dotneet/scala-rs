import scala.language.implicitConversions
class Raw(val n: Int)
class Wrapped(val n: Int)
object Wrapped { implicit def wrap(raw: Raw): Wrapped = new Wrapped(raw.n + 1) }
abstract class Base { def value: Wrapped }
class Child extends Base { def value = new Raw(7) }
abstract class OverBase { def make(x: Any): Wrapped }
class OverChild extends OverBase {
  def make(x: Any): Wrapped = new Wrapped(10)
  def make(x: String) = new Raw(7)
}
abstract class GenericBase[A] { def make(x: A): Wrapped }
class GenericChild extends GenericBase[Int] { def make(x: Int) = new Raw(x) }
abstract class PolyBase { def echo[A](x: A): A }
class PolyChild extends PolyBase { def echo[B](x: B) = x }
object Main { def main(args: Array[String]): Unit = { val base: Base = new Child; println(base.value.n); println(new OverChild().make("x").n); println(new GenericChild().make(10).n); println(new PolyChild().echo(13)) } }
