// Refchecks-level errors: defaults in two overloaded alternatives (their
// getters would share one name) and a value class redefining `AnyVal`'s
// `getClass` without `override`.
object Test {
  def apply(a: Int = 1): Int = a
  def apply(a: String, b: Int = 2): Int = b
}
class A { def f(a: Int = 1): Int = a }
trait T { def f(s: String = ""): Int = 2 }
class B extends A with T
class V(val x: Int) extends AnyVal { def getClass(): Class[_] = null }
