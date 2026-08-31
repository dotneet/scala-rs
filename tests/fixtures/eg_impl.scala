// Macro *implementations*, compiled on their own so that `eg_use.scala` can
// expand against them. `docs/macros.md` §7.11.
//
// nsc requires exactly this split: a macro implementation has to have been
// compiled by an *earlier* run, because expanding means loading its class file
// and running it. `crates/cli/tests/engine.rs` compiles this file first and
// puts the result on the classpath of the second compilation.
//
// Every body here is written in the plain reflect API (`Literal`, `Constant`,
// `Apply`, `Select`, `TermName`), which is what scala-rs can compile today --
// `reify` and `c.Expr[T](tree)` cannot be (§7.8, §7.11), so they are not used.
import scala.reflect.macros.blackbox.Context

object EgHelper {
  def twice(n: Int): Int = n * 2
  def label: String = "helper"
}

object EgImpl {
  // No arguments: the expansion is a bare literal.
  def constImpl(c: Context)(): c.Tree = {
    import c.universe._
    Literal(Constant(42))
  }

  // One `c.Expr[Int]` argument, spliced into the expansion. The operator name
  // arrives NameTransformer-encoded (`$plus`) and has to be decoded again.
  def plusImpl(c: Context)(x: c.Expr[Int]): c.Tree = {
    import c.universe._
    Apply(Select(x.tree, TermName("$plus")), List(Literal(Constant(1))))
  }

  // A raw `c.Tree` argument -- the form nsc has allowed since 2.11 and the one
  // slick's `mapToImpl` uses.
  def twiceImpl(c: Context)(x: c.Tree): c.Tree = {
    import c.universe._
    Apply(Select(Ident(TermName("EgHelper")), TermName("twice")), List(x))
  }

  // A `WeakTypeTag`: the type argument of the macro def reaches the
  // implementation as a tag, and its `tpe` is what the expansion prints.
  def nameOfImpl[T](c: Context)(implicit t: c.WeakTypeTag[T]): c.Tree = {
    import c.universe._
    Literal(Constant(t.tpe.toString))
  }

  // A path the call site does not import: the tree names `EgHelper` and the
  // call site has to resolve it in *its* scope.
  def labelImpl(c: Context)(): c.Tree = {
    import c.universe._
    Select(Ident(TermName("EgHelper")), TermName("label"))
  }
}
