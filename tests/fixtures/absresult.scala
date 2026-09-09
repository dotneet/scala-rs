object Main {
  def main(args: Array[String]): Unit = {
    val base: Base = new Child
    println(base.value.n)
    println(new OverChild().make("x").n)
    println(new GenericChild().make(10).n)
    println(new PolyChild().echo(13))
    val narrow: String = new NarrowChild().value
    println(narrow)
    println(new RecursiveChild().loop(2))
  }
}
class Child extends Base { def value = new Raw(7) }
class OverChild extends OverBase {
  def make(x: Any): Wrapped = new Wrapped(10)
  def make(x: String) = new Raw(7)
}
class GenericChild extends GenericBase[Int] { def make(x: Int) = new Raw(x) }
class PolyChild extends PolyBase { def echo[B](x: B) = x }
class NarrowChild extends Broad { def value = "ok" }
class RecursiveChild extends Recursive { def loop(n: Int) = if (n == 0) "done" else loop(n - 1) }
