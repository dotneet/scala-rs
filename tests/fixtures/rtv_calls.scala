// Call shapes that compiled to code the JVM rejected or misran (fixture
// prefix `rtv_`).
package rtvcalls {
  trait Duh {
    def duh(n: Long) = println("duh " + n + " " + tag)
    def tag: String = "duh"
    val v = 3
  }
  // An inherited member of an enclosing object or package object is called
  // on that singleton, not on `this` (`run/t1987`, `run/t5604`).
  package object inner extends Duh {
    override def tag = "pkg"
  }
  package inner {
    object User {
      def run(): Unit = { duh(1L); println(v) }
    }
  }
  object O extends Duh {
    override def tag = "O"
    object M { def run(): Unit = { duh(2L); println(v) } }
    class K { def run(): Unit = duh(3L) }
    trait S { self: Duh => def srun(): Unit = duh(4L) }
    class SI extends S with Duh { override def tag = "SI" }
  }
  // `B.super[A1].f` from a class nested in `B` runs on `B`'s instance
  // (`run/t10290`).
  trait A1 { def f = "A1" }
  trait A2 { def f = "A2" }
  class B extends A1 with A2 {
    override def f = "B"
    class C {
      def t1 = B.super[A1].f
      def t2 = B.super[A2].f
      def t3 = B.this.f
    }
  }
}

object Main {
  import rtvcalls._
  type P = Int => Unit
  def mk(a: Int)(p: P): P = p
  // A function *value* applied in statement position drops its `BoxedUnit`
  // once (`run/Course-2002-06`).
  def twice(p: P): P = { x: Int => mk(1)(p)(x); mk(2)(p)(x) }
  def h(x: IterableOnce[Int]): Int = x.iterator.size

  def main(args: Array[String]): Unit = {
    inner.User.run()
    O.M.run()
    new O.K().run()
    new O.SI().srun()
    val b = new B
    val c = new b.C
    println(c.t1 + c.t2 + c.t3)
    twice(x => println("p" + x))(7)
    // `Iterator` resolves to the companion even after `IterableOnce.iterator`
    // loaded it through another package (`run/t3269`).
    println(h { println("block"); Iterator.empty })
    // Java varargs take the spliced sequence as an array (`run/t1360`,
    // `run/t3199b`, `run/t4024`).
    val seq: Seq[String] = List("one", "two")
    println(java.util.Arrays.asList(seq: _*))
    println(java.util.Arrays.asList(Seq(1, 2, 3): _*))
    println(java.util.Arrays.asList(Array(1, 2, 3): _*))
    println(String.format("%s-%s", Seq("x", "y"): _*))
    val m = "abc".getClass.getMethod("toString")
    println(m.invoke("abc", Nil: _*))
    // A prelude value class calls a member it only inherits on a real
    // instance (`run/boolord`).
    println(false < true)
    println(true compare false)
    println((3: Int).compareTo(4))
  }
}
