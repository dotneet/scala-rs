// Initialization order of vals in traits and classes: linearization order,
// a trait val read before the class that defines it has run, and a class
// constructor argument observed from a trait body.
object Main {
  val log = new StringBuilder
  def note(s: String): Int = { log.append(s).append(' '); s.length }

  trait A { val a = note("A.a"); note("A.body") }
  trait B extends A { val b = note("B.b"); note("B.body:a=" + a) }
  trait B2 extends A { val b2 = note("B2.b2") }
  class Base { val base = note("Base.base") }
  class C extends Base with B with B2 { val c = note("C.c"); note("C.body") }

  trait X { val x: Int; val y = x + 1 }
  class Y extends X { val x = 10 }
  trait Xs { val s: String; val len = if (s == null) -1 else s.length }
  class Ys extends Xs { val s = "hello" }
  trait Xd { def s: String; val len = if (s == null) -1 else s.length }
  class Yd extends Xd { def s = "hi" }            // a def is never "uninitialized"
  class Yl extends Xs { lazy val s = "lazy!" }     // nor is a lazy val

  abstract class P(val p: Int) { val doubled = p * 2; def show = s"p=$p doubled=$doubled" }
  class Q extends P(21) { val q = doubled + 1 }

  trait Counter { var n = 0; def inc(): Int = { n += 1; n } }
  class UsesCounter extends Counter { val first = inc(); val second = inc() }

  trait T1 { println("T1 init"); def who = "T1" }
  trait T2 extends T1 { println("T2 init"); override def who = "T2>" + super.who }
  trait T3 extends T1 { println("T3 init"); override def who = "T3>" + super.who }
  class M extends T2 with T3 { println("M init") }

  def main(args: Array[String]): Unit = {
    new C
    println(log.toString.trim)
    println(new Y().y)
    println(new Ys().len)
    println(new Yd().len)
    println(new Yl().len)
    val q = new Q
    println(q.show + " q=" + q.q)
    val u = new UsesCounter
    println(s"${u.first} ${u.second} ${u.n}")
    val m = new M
    println(m.who)
  }
}
