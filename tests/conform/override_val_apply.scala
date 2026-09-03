// Minimal shapes the probe round found broken, kept together so one scalac
// run covers them all:
//   * an `override val` / abstract `val` read through the accessor, not the
//     declaring class's field,
//   * an inner class reaching an enclosing method through `$outer`,
//   * `f(x)()` applying the `() => A` a method returned,
//   * a by-name parameter handed on to a local `def` / `lazy val`.
object Main {
  class P2 { val pre: String = "a"; class T2 { def q: String = pre } ; def mk = new T2 }
  class A2 extends P2 { override val pre = "b" }
  abstract class P3 { val pre: String; def show: String = pre + "!" }
  class A3 extends P3 { val pre = "c" }

  class Outer(val tag: String) {
    def deco(s: String): String = "[" + s + "]"
    class Inner(val name: String) { def q(c: String): String = tag + name + deco(c) }
    def make(n: String): Inner = new Inner(n)
  }
  trait Profile {
    def deco(s: String): String
    class Table(val name: String) { def qualified(c: String): String = name + "." + deco(c) }
    def table(n: String): Table = new Table(n)
  }
  object Pg extends Profile { def deco(s: String) = "\"" + s + "\"" }

  def mk(n: Int): () => Int = () => n
  def one[A](a: A): () => A = () => a
  def plain[A](body: => A): () => A = () => body
  def viaLocal[A](body: => A): A = { def go(): A = body; go() }
  def viaLazy[A](body: => A): A = { lazy val v = body; v }
  def once[A](body: => A): () => A = { lazy val v = { println("forced"); body }; () => v }

  def main(a: Array[String]): Unit = {
    println(new P2().mk.q)
    println(new A2().mk.q)
    println((new A2(): P2).pre)
    println(new A3().show)
    println((new A3(): P3).pre)

    val o = new Outer("o")
    println(o.make("m").q("c"))
    println(new o.Inner("i").q("d"))
    println(Pg.table("t").qualified("c"))

    println(mk(3)())
    println(one("s")())
    println(plain("t")())
    println(viaLocal({ println("l"); 1 }))
    println(viaLazy({ println("z"); 2 }))
    val f = once { println("body"); 41 + 1 }
    println("before")
    println(f())
    println(f())
  }
}
