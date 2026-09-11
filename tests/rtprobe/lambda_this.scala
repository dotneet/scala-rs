// Lambdas inside classes and traits capturing `this` and its fields,
// lambdas created during construction, lambdas that outlive the frame,
// and lambdas in secondary constructors and object initializers.
object Main {
  class Counter(start: Int) {
    var n = start
    val incr: () => Int = () => { n += 1; n }
    val readLater = List(1, 2).map(i => () => n * i)
    def adder(k: Int): Int => Int = x => x + k + n
    def this() = { this(100); n += 1 }
  }
  trait Greeter { def name: String; def greeter: String => String = s => s"$s from $name" }
  class G(val name: String) extends Greeter
  object Registry { val names = scala.collection.mutable.ListBuffer.empty[String]; val add: String => Unit = names += _; add("init") }
  class Chain(val v: Int) { def map(f: Int => Int): Chain = new Chain(f(v)); def viaThis: Chain = map(_ + v) }
  abstract class Base { val hook: Int => Int; def use(x: Int) = hook(x) }
  class Impl(k: Int) extends Base { val hook = (x: Int) => x * k }
  def main(args: Array[String]): Unit = {
    val c = new Counter(1)
    c.incr(); c.incr()
    println(c.n + " " + c.readLater.map(_()) + " " + c.adder(10)(1))
    c.n = 50
    println(c.readLater.map(_()) + " " + c.adder(0)(0))
    val c2 = new Counter()
    println(c2.n + " " + c2.incr())
    println(new G("g").greeter("hello"))
    Registry.add("later")
    println(Registry.names)
    println(new Chain(3).viaThis.viaThis.v)
    println(new Impl(7).use(6))
    val fs = (1 to 3).map(i => new Counter(i)).map(_.incr)
    println(fs.map(_()))
  }
}
