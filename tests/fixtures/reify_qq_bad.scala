// The forms reification does *not* build yet are named, one by one.
//
// A quasiquote that silently built the wrong tree would be far worse than one
// that does not compile: the call site would typecheck against a tree nobody
// wrote. So every gap in `crates/typer/src/reify.rs` is an error that says
// which form is missing.
import scala.reflect.runtime.universe._

object Main {
  def main(args: Array[String]): Unit = {
    // Forms with no `Syntactic*` lowering here yet.
    println(q"{ val a = 1; a }")
    println(q"new Foo(1)")
    println(q"(x: Int) => x")
    println(q"if (a) b else c")
    // A splice may stand for a whole argument list, but mixing it with
    // ordinary arguments needs a concatenation this does not build.
    val xs = List(q"p")
    println(q"k(1, ..$xs)")
    // A `..$` hole is a list; it cannot stand where a single tree goes.
    println(q"..$xs")
  }
}
