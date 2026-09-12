// Types in a `reify { … }` body: a type parameter with no tag in scope is a
// *free type* (and the toolbox says so, naming where it was defined), one
// with a tag is spliced from the tag, and a written type is rebuilt from the
// class it resolved to. `crates/cli/tests/reify2.rs` compares every line with
// real scalac 2.13.16.
import scala.reflect.runtime.universe._
import scala.tools.reflect.{ToolBox, ToolBoxError}
import scala.tools.reflect.Eval

object Main extends App {
  class C[T] {
    val code = reify { List[T](2.asInstanceOf[T]) }
    def run(): Unit =
      try println(code.eval)
      catch { case e: ToolBoxError => println(e.getMessage) }
  }
  new C[Int].run()

  def tagged[T: TypeTag](t: T): Expr[List[T]] = reify { List[T](t) }
  println(tagged(7).eval)
  println(tagged("s").staticType)
  println(showRaw(tagged(7).tree))

  val written = reify {
    val r: AnyRef = "s"; val a: Any = 2; val s: String = "x"; val o: Option[Int] = Some(1)
    val f: Int => Int = i => i + 1
    (r, a, s, o, f(1))
  }
  println(written.eval)
  println(showRaw(written.tree))

  // A local class in a type position, and an ascription to one.
  val local = reify { class L(val n: Int); val l: L = new L(3); (l: L).n }
  println(local.eval)
  println(local.staticType)
}
