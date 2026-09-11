// Overriding generic methods at primitive and concrete types: bridges must
// route calls through the erased signature to the override, including
// covariant result types and trait-implemented generic methods.
object Main {
  trait Conv[A, B] { def conv(a: A): B }
  object IntToStr extends Conv[Int, String] { def conv(a: Int) = "#" + a }
  class DoubleIt extends Conv[Double, Double] { def conv(a: Double) = a * 2 }
  abstract class Animal { def self: Animal = this; def name = "animal" }
  class Cat extends Animal { override def self: Cat = this; override def name = "cat"; def meow = "meow" }
  trait Source[+A] { def get: A }
  class IntSource(v: Int) extends Source[Int] { def get: Int = v }
  trait Sink[-A] { def put(a: A): String }
  class AnySink extends Sink[Any] { def put(a: Any) = "sank " + a }
  abstract class Cmp[T] { def compare(a: T, b: T): Int; def max(a: T, b: T): T = if (compare(a, b) >= 0) a else b }
  class IntCmp extends Cmp[Int] { def compare(a: Int, b: Int) = a - b }
  class LenCmp extends Cmp[String] { def compare(a: String, b: String) = a.length - b.length }
  trait Fn1[-A, +B] { def apply(a: A): B }
  class Neg extends Fn1[Int, Int] { def apply(a: Int) = -a }
  class Box[T](t: T) { def get: T = t }
  class IntBox(i: Int) extends Box[Int](i) { override def get: Int = super.get + 1000 }
  trait Visitor[R] { def visitNum(n: Int): R; def visitStr(s: String): R }
  object Printer extends Visitor[String] { def visitNum(n: Int) = "num:" + n; def visitStr(s: String) = "str:" + s }
  object Summer extends Visitor[Int] { def visitNum(n: Int) = n; def visitStr(s: String) = s.length }
  def accept[R](v: Visitor[R], items: List[Either[Int, String]]): List[R] = items.map(_.fold(v.visitNum, v.visitStr))
  def main(args: Array[String]): Unit = {
    val c: Conv[Int, String] = IntToStr; println(c.conv(5))
    val d: Conv[Double, Double] = new DoubleIt; println(d.conv(1.25))
    val a: Animal = new Cat; println(a.self.name + " " + new Cat().self.meow)
    val s: Source[Any] = new IntSource(7); println(s.get)
    val si: Sink[String] = new AnySink; println(si.put("x"))
    println(new IntCmp().max(3, 9) + " " + new LenCmp().max("aaa", "bb"))
    val cmp: Cmp[Int] = new IntCmp; println(cmp.compare(10, 4))
    val f: Fn1[Int, Any] = new Neg; println(f(5))
    val b: Box[Int] = new IntBox(1); println(b.get + " " + new IntBox(2).get)
    val items = List(Left(3), Right("four"), Left(5))
    println(accept(Printer, items) + " " + accept(Summer, items).sum)
    val fs: List[Conv[Int, String]] = List(IntToStr, new Conv[Int, String] { def conv(a: Int) = "anon" + a })
    println(fs.map(_.conv(1)))
  }
}
