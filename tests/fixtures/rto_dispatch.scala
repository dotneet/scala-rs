// Which member a call reaches and what an expression evaluates, checked
// against scalac 2.13.16 (crates/cli/tests/rto.rs).
import scala.reflect.ClassTag

object O {
  println("init O")
  case class N()
  object P { println("init P"); def i = "P.i" }
}

abstract class Delayed(a: Int) extends scala.DelayedInit {
  def delayedInit(x: => Unit): Unit = { println("delayed init " + a); x }
}
class Sub extends Delayed(2) { println("sub body") }

object Main {
  // A non-method alternative compatible with a function type beats
  // eta-expansion of the method (run/t9395).
  def f(s: String): String = "method"
  val f: String => String = s => "value"

  // Weak conformance is applicability: `Double` is more specific (run/t12560).
  def g(x: AnyVal) = "AnyVal"
  def g(x: Double) = "Double"
  def k(x: Any) = "Any"
  def k(x: Long) = "Long"

  class C

  def main(args: Array[String]): Unit = {
    // A qualifier that is not a stable path still runs (run/t4859).
    O.P.i
    println({ println("side effect"); O }.P.i)
    val n = { println("again"); O }.N
    println(n)

    val t: String => String = f
    println(t(""))
    println((g(3), g('a'), g(3.0), g(true), k(3), k('c')))

    println(classOf[(String, Int)])
    println(classOf[Int => Boolean])
    val z: Class[C] = classOf
    println(z)
    println((implicitly[ClassTag[AnyVal]] eq ClassTag.AnyVal, implicitly[ClassTag[AnyVal]]))

    new Delayed(1) { println("anon body") }
    new Sub
  }
}
