// Everything that is *not* object-private is still variance-checked: scalac
// reports all seven of these.
object Main {
  class A[+T](private var st: T => Int)
  class B[+T] { private def take(t: T): Int = 0 }
  class C[+T] { private[Main] def take(t: T): Int = 0 }
  class D[+T] { protected def take(t: T): Int = 0 }
  class E[+T] { def take(t: T): Int = 0 }
  class F[-T] { private def give: T = ??? }
  class G[+T] { private[this] var ok: T => Int = _ => 0; private var bad: T => Int = _ => 0 }
  def main(args: Array[String]): Unit = ()
}
