// A case class value class's companion `apply` erases to the identity over
// its underlying representation (`(I)I`, not `(I)LWrapped;`), because the
// class itself erases to that same underlying type. `emit_case_apply`
// (crates/backend/src/gen.rs) always wrote that narrow descriptor on the
// classfile, but our own call sites read a *different*, boxed descriptor off
// the method symbol -- `Wrapped$.apply` structurally overrides
// `AbstractFunctionN.apply` (the companion extends it so it can serve as a
// function value), and the general "our primitive narrows the overridden's
// Object" erasure rule widened the symbol's own stored return type back to
// `Object` to match. `NoSuchMethodError: 'java.lang.Object Wrapped$.apply(int)'`
// followed, invisible to every static check because the verifier does not
// resolve method descriptors against anything but the constant pool.
//
// Covers a `case class ... extends AnyVal` via the companion `apply` and via
// `new`, and a non-case-class value class via `new` and its own extension
// method, in both jar (`--scala-library`) and private-runtime modes.
final case class Wrapped(u: Int) extends AnyVal
class Plain(val u: Int) extends AnyVal {
  def inc: Int = u + 1
}

object Main {
  def main(args: Array[String]): Unit = {
    val w1 = Wrapped(7)
    val w2 = new Wrapped(9)
    println(w1.u)
    println(w2.u)
    println(w1 == w2)
    println(w1 == Wrapped(7))
    println(w1.toString)

    val p = new Plain(3)
    println(p.u)
    println(p.inc)
  }
}
