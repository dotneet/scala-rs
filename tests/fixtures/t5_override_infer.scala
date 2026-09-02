// An `override def` that omits its own result type takes the overridden
// member's, exactly as SLS 6.1's "if... the definition does not specify a
// type, the type... is assumed to be the type of the member that is
// overridden" says -- so a self-recursive call inside its own body does not
// need the "recursive method X needs result type" cycle a *standalone*
// method of the same shape (`t5_override_infer_bad.scala`) still reports:
// the overridden type is known before the body is typed at all.
//
// `type_def_sig` now looks the overridden member up (`overridden_ret_type`,
// gated on the written `override` modifier) and borrows its return type
// when the override itself has none written. Only the return type is
// borrowed; the body is still checked/inferred exactly as written.
//
// A second, independent bug sat behind this one: a method whose signature
// this way became "known" still stayed registered as needing lazy, on-demand
// completion (`pending_sigs`), since that bookkeeping only ever looked at
// the parsed syntax (no written `: T`), never at whether a type had already
// been produced some other way. A *self*-reference inside such a method's
// own body then found itself still "pending", ran `complete_lazy_sig` on
// itself, and that locked the symbol and re-entered body-typing on a cloned
// copy of the very body already being typed -- whose own self-reference
// then found the symbol locked and reported the cycle anyway, even though
// the return type was never actually in question. `register_typed_sig` now
// also treats a `DefDef` whose signature pass already produced a real
// return type as no longer lazy.
object Main {
  trait Node
  case class Wrap(inner: Node) extends Node
  case object Leaf extends Node

  class Base {
    def run(n: Node): Any = n match {
      case Wrap(x) => run(x)
      case Leaf => "leaf"
    }
  }
  class Sub extends Base {
    override def run(n: Node) = n match {
      case Wrap(x) => run(x).asInstanceOf[String] + "!"
      case n => super.run(n)
    }
  }

  def main(args: Array[String]): Unit = {
    println(new Sub().run(Wrap(Leaf)))
  }
}
