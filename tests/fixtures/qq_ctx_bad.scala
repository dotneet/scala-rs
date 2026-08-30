// What a macro implementation may *not* do yet, all of it diagnosed.
//
// Now that `c.universe` resolves and quasiquotes inside a macro
// implementation are reified, the failure that matters is the quiet one: a
// form reification does not build must never come out as some other tree.
// Every quasiquote below is reported, naming the form -- see
// `crates/typer/src/reify.rs` and `docs/macros.md` §7.5.
import scala.reflect.macros.blackbox

object QqCtxBad {
  // A type ascription: the shape slick's `ShapedValue.mapToImpl` uses eight
  // times over (`q"($rModule.apply _) : ($uTag => $rTag)"`).
  def ascribed(c: blackbox.Context)(x: c.Tree): c.Tree = {
    import c.universe._
    q"$x : Int"
  }

  // A block.
  def block(c: blackbox.Context)(x: c.Tree): c.Tree = {
    import c.universe._
    q"""
      val a = $x
      a
    """
  }

  // `tq"..."`, a type quasiquote: parsed, not reified.
  def typeQuote(c: blackbox.Context): c.Tree = {
    import c.universe._
    tq"scala.Int"
  }

  // A hole whose argument is not a tree. nsc lifts it with an implicit
  // `Liftable`; we do not, so this is a type error rather than a wrong tree.
  def unlifted(c: blackbox.Context)(n: Int): c.Tree = {
    import c.universe._
    q"f($n)"
  }
}
