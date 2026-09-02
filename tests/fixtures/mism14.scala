// agent/mismatch14: three roots behind slick's remaining `no matching
// overload` / `type mismatch` reports.
//
// 1. A *monomorphic* callee's parameter type is the expected type of its
//    argument, so a function literal that sits inside the argument (an
//    if/else, a block) can read its parameter types off it. Only the
//    one-expression body ever worked, and only by accident: the typer
//    recovered the parameter from the call inside it.
// 2. An implicit conversion whose parameter is a generic *supertype* of the
//    receiver is solved against the receiver's base type at that class.
// 3. `Any` written for a *Java* method's type parameter means the `Object`
//    the parameter is really bounded by (nsc's `ObjectTpeJava`).
import java.util.Arrays

// `Cfg.Params(…)` is a *companion* apply: the callee is the module, and its
// `apply` sits next to the `AbstractFunction3.apply` the companion inherits.
// This is slick's `JdbcBackend.StatementParameters(…)`.
object Cfg {
  case class Params(name: String, init: String => Unit, size: Int)
}

object Main {
  def twice(f: String => Unit): Unit = { f("a"); f("b") }

  def one(s: String): Unit = print(s)
  def two(s: String): Unit = { print(s); print(s) }

  trait Sink { def accept(s: String): Unit }
  def drain(k: Sink): Unit = k.accept("z")

  trait Holder[A, B] {
    def first: A
    def second: B
  }
  class SI extends Holder[String, Int] {
    def first = "si"
    def second = 7
  }
  implicit class HolderOps[A, B](val h: Holder[A, B]) {
    def firstOf: A = h.first
    def pair: (A, B) = (h.first, h.second)
  }

  // An inherited result type that mentions an abstract type member is read
  // through the class that inherits it: inside `Leaf`, `Self` is `Leaf`.
  trait Tree {
    type Self >: this.type <: Tree
    protected[this] def rebuild(n: Int): Self
    def label: String
    def grown: Tree = rebuild(1)
  }
  final case class Leaf(name: String) extends Tree {
    type Self = Leaf
    override protected[this] def rebuild(n: Int) = Leaf(name + n.toString)
    def label = name
  }

  def merge(flag: Boolean, prev: Cfg.Params): Cfg.Params =
    Cfg.Params(
      "p",
      if (flag) prev.init
      else { s => one(s); two(s) },
      3
    )

  def grow(a: Array[Any]): Array[Any] =
    Arrays.copyOf[Any](a.asInstanceOf[Array[AnyRef]], a.length + 1).asInstanceOf[Array[Any]]

  def main(args: Array[String]): Unit = {
    val flag = args.length == 0
    // The else branch has a two-statement body: nothing inside it says what
    // `s` is, only `twice`'s parameter type does.
    twice(if (flag) { s => two(s); one(s) } else { s => one(s) })
    println()
    // The same through a single-abstract-method parameter.
    drain(if (flag) { s => print(s); print(s) } else { s => print(s) })
    println()
    val si = new SI
    val f: String = si.firstOf
    val p: (String, Int) = si.pair
    println(f + " " + p._1 + " " + p._2)
    val a: Array[Any] = new Array[Any](2)
    a(0) = "x"
    a(1) = "y"
    println(Leaf("leaf").grown.label)
    merge(flag, Cfg.Params("q", one, 1)).init("m")
    merge(!flag, Cfg.Params("q", one, 1)).init("n")
    println()
    val g = grow(a)
    println(g.length.toString + " " + g(0) + " " + g(2))
  }
}
