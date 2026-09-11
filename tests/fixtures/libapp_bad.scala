// Programs scalac rejects next to the ones the `libapp_` repairs accept.
// Every error line is listed in `crates/cli/tests/libapp.rs`.
package scala.collection.libappbad {
  final class Node[K, V](val key: K, val value: V) {
    def foreach[U](f: ((K, V)) => U): Unit = f((key, value))
  }
  object UseNode {
    def a[A, B](n: Node[A, B]): Unit = n.foreach((k: A) => ())
    def b[A, B](n: Node[A, B], g: A => Unit): Unit = n.foreach(g)
  }
}

package libappbad {
  class V1 { val value: Int = 1; def value_=(n: Int): Unit = () }
  class V2 { private var n = 0; def x(): Int = n; def x_=(v: Int): Unit = n = v }
  class V3 { def y: Int = 1 }

  class Box[A] {
    var a: A = null
    def f(): Unit = { var local: A = _; () }
  }

  class Narrow(val b: Byte)
  class Inv(val xs: Array[Any])

  trait Num[T] { class Ops(lhs: T) }
  object StrNum extends Num[String]
  trait Integ[T] extends Num[T] {
    class Bad(x: String) extends Ops(x)
  }

  trait Bx[A] { def get: A }
  trait Hd[B] extends Bx[B] {
    class Inner { def first: Int = get }
  }

  object Main {
    def main(args: Array[String]): Unit = {
      java.util.Arrays.fill(new Array[String](2), "x")
      java.util.Arrays.deepToString(new Array[Int](2))
      val v1 = new V1
      v1.value += 1
      val v2 = new V2
      v2.x += 1
      val v3 = new V3
      v3.y += 1
      new Narrow(300)
      val as: Array[AnyRef] = Array("a")
      new Inv(as)
      new StrNum.Ops(3)
    }
  }

  // `var input: In = _` compiles now, so its setter written a second time
  // has to be reported for itself (`neg/t591`).
  trait Flow {
    type In
    private var input: In = _
    def input_=(in: In) = {}
  }
}
