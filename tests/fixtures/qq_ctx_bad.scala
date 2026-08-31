// What a macro implementation may *not* do yet, all of it diagnosed.
//
// Now that `c.universe` resolves and quasiquotes inside a macro
// implementation are reified, the failure that matters is the quiet one: a
// form reification does not build must never come out as some other tree.
// Every quasiquote below is reported, naming the form -- see
// `crates/typer/src/reify.rs` and `docs/macros.md` §7.7.
//
// The forms this file used to hold -- a type ascription, a block, `tq"..."` --
// are reified now (`qq_ctx.scala` writes them, `qr_forms.scala` checks the
// trees against real scalac). What is left is the set the *parser* normalises
// something away from, where building anything would build a tree nobody
// wrote.
import scala.reflect.macros.blackbox

object QqCtxBad {
  // The parser supplies `()` for a missing `else`; nsc supplies an empty
  // block.
  def ifNoElse(c: blackbox.Context): c.Tree = {
    import c.universe._
    q"if (a) b"
  }

  // A right-associative operator and a `_` placeholder lambda used to stand
  // here; both are reified now, out of the `freshTermName` block nsc builds
  // for them (`tests/fixtures/fn2_fresh.scala`, `docs/macros.md` §7.10).

  // A by-name type: nsc's own parser rejects it inside `tq"..."`.
  def byNameType(c: blackbox.Context): c.Tree = {
    import c.universe._
    tq"=> Int"
  }

  // A hole whose argument is neither a tree nor anything with a *standard*
  // `Liftable` (`docs/macros.md` §7.8). nsc would look for a user-written
  // `Liftable[File]` implicit; scala-rs only knows the standard instances, so
  // it names the type it cannot lift rather than building some other tree.
  // (`Int`, which used to stand here, is lifted now.)
  def unlifted(c: blackbox.Context)(n: java.io.File): c.Tree = {
    import c.universe._
    q"f($n)"
  }
}
