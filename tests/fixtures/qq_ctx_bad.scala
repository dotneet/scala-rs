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
  // `a :: b` is `b.::(a)` once parsed, indistinguishable from a written
  // `b.::(a)`; nsc builds neither, it binds the left operand to a fresh `val`
  // first so evaluation order is kept.
  def rightAssoc(c: blackbox.Context): c.Tree = {
    import c.universe._
    q"a :: b"
  }

  // The parser supplies `()` for a missing `else`; nsc supplies an empty
  // block.
  def ifNoElse(c: blackbox.Context): c.Tree = {
    import c.universe._
    q"if (a) b"
  }

  // `_.get` is a lambda over a parameter the parser invented; nsc names that
  // parameter with `freshTermName`.
  def placeholder(c: blackbox.Context): c.Tree = {
    import c.universe._
    q"_.get"
  }

  // A by-name type: nsc's own parser rejects it inside `tq"..."`.
  def byNameType(c: blackbox.Context): c.Tree = {
    import c.universe._
    tq"=> Int"
  }

  // A hole whose argument is not a tree. nsc lifts it with an implicit
  // `Liftable`; we do not, so this is a type error rather than a wrong tree.
  def unlifted(c: blackbox.Context)(n: Int): c.Tree = {
    import c.universe._
    q"f($n)"
  }
}
