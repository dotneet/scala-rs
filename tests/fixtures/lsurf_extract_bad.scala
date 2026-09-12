// Extractors scalac 2.13.16 rejects (neg/t7850, neg/t8989, neg/t5078).
class Casey(a: Int) { def isEmpty = this; def get = this }
object Casey { def unapply(a: Casey) = a }

class Dingy(a: Int) { def get = this }
object Dingy { def unapply(a: Dingy) = a }

class NoGet extends Product1[Int] {
  def _1 = 1
  def isEmpty = false
  def canEqual(a: Any) = true
}
object NoGet { def unapply(a: Any) = new NoGet }

object Foo { def unapply: Option[Int] = Some(42) }
object Foo2 { def unapply(): Option[Int] = Some(42) }
object Foo3 { def unapply(a: Int, b: Int): Option[Int] = Some(42) }

object Main {
  def main(args: Array[String]): Unit = {
    val Casey(x1) = new Casey(1)
    val Dingy(x2) = new Dingy(1)
    val p: Any = 1
    p match { case NoGet(1) => (); case _ => () }
    val Foo(x3) = 1
    val Foo2(y3) = 2
    val Foo3(z3) = 3
    println(s"$x1 $x2 $x3 $y3 $z3")
  }
}
