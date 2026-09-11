// Members selected through a type projection `Base#Inner` (`neg/sabin2`'s
// accepting neighbours) and through an object's inherited inner class
// (`IntBase.Inner`). The output is compared with scalac.
object Test {
  abstract class Base {
    type T
    var x: T = _
    class Inner {
      def set(y: T): Unit = x = y
      def get(): T = x
      def id(y: T): T = y
      def lst: List[T] = Nil
      def f(g: T => Unit): Unit = g(x)
      def pick(y: T): String = "T"
      def pick(y: String): String = "String"
      def print(): Unit = println("Hello world")
    }
    def m(i: Inner, t: T): Unit = i.set(t)
  }
  object IntBase extends Base { type T = Int }
  object StringBase extends Base { type T = String }
  class StrBase extends Base { type T = String }

  abstract class Fixed {
    type T = Int
    class Inner { def set(y: T): Unit = (); def get(): T = 1 }
  }

  val a: Base#Inner = new IntBase.Inner
  val b: Base#Inner = new StringBase.Inner
  val c: IntBase.Inner = new IntBase.Inner
  val e: IntBase.type#Inner = new IntBase.Inner
  val sb: StrBase = new StrBase
  val g: StrBase#Inner = new sb.Inner
  val h: sb.Inner = new sb.Inner
  object FixedObj extends Fixed
  val f: Fixed#Inner = new FixedObj.Inner
  val x: Base = IntBase
  val i: x.Inner = new x.Inner

  def run(): Unit = {
    a.print()
    b.print()
    val t: Base#T = a.get()
    val l: List[Base#T] = a.lst
    c.set(1)
    val ci: Int = c.get()
    e.set(3)
    g.set("g")
    h.set("h")
    val s: String = g.get()
    f.set(1)
    val j: Int = f.get()
    val v: x.T = i.get()
    i.set(v)
    a.f((u: Base#T) => println("f " + u))
    println(a.pick("s"))
    val n = new IntBase.Inner
    n.set(4)
    IntBase.m(n, 5)
    println(List(ci, j, n.get()))
    println(List(t, l, s))
    try a.set(???) catch { case _: NotImplementedError => println("???") }
  }
}

object Main { def main(args: Array[String]): Unit = Test.run() }
