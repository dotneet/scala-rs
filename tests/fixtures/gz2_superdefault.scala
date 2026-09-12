// `super.m(x)` on a method with a defaulted parameter: the omitted argument is
// `super.m$default$2`, so the class mixing the trait in owes a super accessor
// for the *default getter* as well. nsc emits
// `C.B$$super$m$default$2()` forwarding to `A.m$default$2$(A)`; the getter is
// synthesized as a symbol with its body in `Symbol::default_rhs`, never as a
// `DefDef` in the trait's body, so the backend has to find it in the symbol
// table. gitbucket's fourteen controllers reach this through `RequestCache`'s
// `super.getAccountByUserName(userName)`.
trait Gz2Base {
  def m(x: String, loud: Boolean = false): String = if (loud) x.toUpperCase else x
  def k(n: Int = 7): Int = n * 2
}

trait Gz2Cache extends Gz2Base {
  def cached(x: String): String = "[" + super.m(x) + "]"
  def cachedK: Int = super.k()
  override def m(x: String, loud: Boolean = false): String = "<" + x + ">"
}

class Gz2Client extends Gz2Cache

object Gz2SuperDefault {
  def main(args: Array[String]): Unit = {
    val c = new Gz2Client
    println(c.cached("ab"))
    println(c.cachedK)
    println(c.m("ab"))
    println(c.m("ab", loud = true))
  }
}
