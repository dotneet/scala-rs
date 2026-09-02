// The control for `t5_override_infer.scala`: the identical self-recursive
// body, but on a method that overrides nothing. There is no overridden
// signature to borrow a return type from, so this is still nsc's ordinary
// "recursive method X needs result type" (confirmed against scalac
// 2.13.16) -- the fix must not turn *every* unannotated recursive method
// into a non-error, only ones that genuinely override an already-typed
// member.
object Main {
  trait Node
  case class Wrap(inner: Node) extends Node
  case object Leaf extends Node

  class Standalone {
    def run(n: Node) = n match {
      case Wrap(x) => run(x).asInstanceOf[String] + "!"
      case Leaf => "leaf"
    }
  }

  def main(args: Array[String]): Unit = println(new Standalone().run(Wrap(Leaf)))
}
