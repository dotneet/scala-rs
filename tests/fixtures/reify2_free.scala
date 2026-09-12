// `reify { … }` over the typed body (`docs/notes/reify-design.md`): locals
// and parameters bound outside the body travel as free terms, members of the
// enclosing `object` through `mkThis`, and everything the body defines itself
// -- classes, objects, defs, vals, vars, closures, pattern binders -- by name.
// Every expression is evaluated by the toolbox, and the two trees whose shape
// is printed are compared against real scalac 2.13.16 by `crates/cli/tests/
// reify2.rs`.
import scala.reflect.runtime.universe._
import scala.tools.reflect.ToolBox
import scala.tools.reflect.Eval

object Reify2Helper {
  def twice(n: Int): Int = n * 2
  val four = 4
}

object Main extends App {
  val member = 10
  class Box(val n: Int)
  object Inner { val q = 5 }

  // A parameter, a local `val`, a local parameterless `def`, a local `lazy
  // val`: free terms. `member` and `Inner` are reached through `Main.this`,
  // `Box` is a class of the enclosing object.
  def free(x: Int): Expr[Int] = {
    val y = 3
    def g = 5
    lazy val lz = 6
    reify { x + y + g + lz + member + Inner.q + new Box(1).n }
  }
  // A `var` bound outside the body is carried by value: reads are fine.
  def freeVar(): Expr[Int] = {
    var w = 4
    w += 1
    reify { w * 2 }
  }
  // A closure over a parameter the body itself binds.
  val closure = reify { (i: Int) => i + Reify2Helper.twice(Reify2Helper.four) }
  // Definitions inside the body, a `while`, assignments to a local `var`.
  val defs = reify {
    class C { def m = 1; val v = 2 }
    object O { val z = 3 }
    def f(a: Int): Int = a * 2
    val c = new C
    var acc = 0
    var i = 0
    while (i < 3) { acc += f(i); i += 1 }
    acc + c.m + c.v + O.z
  }
  // A pattern `val` (reified in its desugared form, which differs from
  // nsc's spelling of the same desugaring, so only evaluated), and an
  // extractor pattern with a typed binder and a guard.
  val pat = reify {
    val (a, b) = (1, 2)
    (Some(a): Option[Int]) match { case Some(n: Int) if n > 0 => n + b; case _ => 0 }
  }
  val extractor = reify {
    (Some(1): Option[Int]) match { case Some(n: Int) if n > 0 => n; case _ => 0 }
  }
  // Definitions with written types and a type parameter of their own.
  val typed = reify { val xs: List[Int] = List(1, 2, 3); def id[U](x: U): U = x; id(xs.size) }
  val tryCatch = reify {
    try { throw new Exception("boom") } catch { case e: Exception => e.getMessage.length } finally ()
  }
  // String interpolation, a symbol literal, a right-associative operator.
  val strings = reify { val s = "ab"; s"<$s:${s + "!"}>" + 'sym.name + (1 :: Nil).head }
  // A `reify` nested in the body, evaluated there: `x` is local to the outer.
  val nested = reify { val x = 2; reify { x }.eval }
  // A `for` comprehension: reified in its desugared form, as nsc does.
  val forLoop = reify { (for (i <- List(1, 2, 3); if i % 2 == 1) yield i * 10).sum }
  // An anonymous class.
  val anon = reify { new { def x = 2; def y = x * x }.y }

  println(free(1).eval)
  println(freeVar().eval)
  println(closure.eval.apply(34))
  println(defs.eval)
  println(pat.eval)
  println(tryCatch.eval)
  println(strings.eval)
  println(nested.eval)
  println(forLoop.eval)
  println(anon.eval)
  println(extractor.eval)
  println(typed.eval)
  println(showRaw(closure.tree))
  println(showRaw(free(1).tree))
  println(showRaw(extractor.tree))
  println(showRaw(strings.tree))
  println(showRaw(typed.tree))
}
