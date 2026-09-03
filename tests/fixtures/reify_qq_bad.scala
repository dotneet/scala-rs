// The forms reification does *not* build yet are named, one by one.
//
// A quasiquote that silently built the wrong tree would be far worse than one
// that does not compile: the call site would typecheck against a tree nobody
// wrote. So every gap in `crates/typer/src/reify.rs` is an error that says
// which form is missing.
//
// The forms this file used to list -- a block, `new`, a function literal,
// `if` -- are built now; `tests/fixtures/qr_forms.scala` checks the trees
// they produce against real scalac. What is left here is the set where the
// *parser* has already normalised away something nsc keeps, so there is
// nothing honest to build.
import scala.reflect.runtime.universe._

object Main {
  def main(args: Array[String]): Unit = {
    // The parser supplies `()` for a missing `else`; nsc supplies an empty
    // block.
    println(q"if (a) b")
    // `a :: b` and `_.get` used to stand here; both are reified now, out of
    // the `freshTermName` block nsc builds for them
    // (`tests/fixtures/fn2_fresh.scala`, `docs/macros.md` §7.10).
    // Mixing a splice with ordinary arguments is built now, the way nsc's own
    // `reifyList` builds it (`docs/macros.md` §7.16); the line is kept because
    // the *shape* still has to compile. What is refused is a rank-2
    // `...$xss`, in `tests/fixtures/sv_gaps_bad.scala`.
    val xs = List(q"p")
    println(q"k(1, ..$xs)")
    // A `..$` hole is a list; it cannot stand where a single tree goes.
    println(q"..$xs.size")
  }
}
