// Macro *implementations* that return `c.Expr[T]` and read `c.prefix`.
// `docs/macros.md` §7.12. Compiled on its own so `ex_use.scala` can expand
// against it, the split nsc requires (§1.3).
//
// `eg_impl.scala` had to write every body as a bare `c.Tree`, because
// `c.Expr[T](tree)` did not resolve to `Context.Expr` -- it landed on
// `universe.Expr.apply`, whose parameters are `(Mirror, TreeCreator)`. slick's
// `TableQueryMacroImpl.apply` returns a `c.Expr`, so that had to be fixed
// before any of slick's macros could be written at all.
import scala.reflect.macros.blackbox.Context

class ExRow(val tag: String) {
  override def toString: String = "row(" + tag + ")"
}

class ExBox[E](val row: E) {
  override def toString: String = "box(" + row.toString + ")"
}

object ExBox {
  def of[E](row: E): ExBox[E] = new ExBox[E](row)
}

object ExImpl {
  // The plain shape: an `Expr` built out of a literal.
  def constImpl(c: Context)(): c.Expr[Int] = {
    import c.universe._
    c.Expr[Int](Literal(Constant(7)))
  }

  // An `Expr` argument spliced into an `Expr` result. The operator name
  // arrives NameTransformer-encoded.
  def doubleImpl(c: Context)(x: c.Expr[Int]): c.Expr[Int] = {
    import c.universe._
    c.Expr[Int](Apply(Select(x.tree, TermName("$times")), List(Literal(Constant(2)))))
  }

  // `c.prefix` -- the receiver of the macro application. slick's `mapToImpl`
  // is built on this: the macro is a method on a class and the expansion has
  // to name the object it was called on.
  def tagImpl(c: Context): c.Expr[String] = {
    import c.universe._
    c.Expr[String](Select(c.prefix.tree, TermName("tag")))
  }

  // The prefix used twice, and as a receiver for a call rather than a field.
  def bothImpl(c: Context)(other: c.Expr[ExRow]): c.Expr[String] = {
    import c.universe._
    c.Expr[String](
      Apply(
        Select(Select(c.prefix.tree, TermName("tag")), TermName("$plus")),
        List(Select(other.tree, TermName("tag")))
      )
    )
  }

  // The shape of slick's `TableQueryMacroImpl.apply`: a `c.Expr[F[E]]` result
  // built from a `WeakTypeTag[E]`, whose `tpe` becomes a `TypeTree` under a
  // `New`. slick writes `Apply(Select(New(TypeTree(e.tpe)), termNames.CONSTRUCTOR),
  // List(Ident(TermName("tag"))))` for the row it constructs; this is the same
  // tree with a literal for the tag.
  def boxImpl[E](c: Context)(implicit e: c.WeakTypeTag[E]): c.Expr[ExBox[E]] = {
    import c.universe._
    val row = Apply(
      Select(New(TypeTree(e.tpe)), termNames.CONSTRUCTOR),
      List(Literal(Constant("made")))
    )
    c.Expr[ExBox[E]](Apply(Select(Ident(TermName("ExBox")), TermName("of")), List(row)))
  }

  // The prefix's *tag*, not its tree. nsc builds `c.prefix` as
  // `Expr[Nothing](prefixTree)(TypeTag.Nothing)` -- `PrefixType` is an
  // abstract member of the blackbox `Context`, so the receiver's real type is
  // not in there. This prints `Nothing` under both compilers, which is what
  // pins that the bridge does not invent a type of its own.
  def prefixTypeImpl(c: Context): c.Expr[String] = {
    import c.universe._
    c.Expr[String](Literal(Constant(c.prefix.staticType.toString)))
  }

  // The composed tag made visible. `c.Expr[ExBox[E]]` above needs an implicit
  // `WeakTypeTag[ExBox[E]]`, which no program defines: it is materialised out
  // of a `staticClass` for `ExBox`, `appliedType`, and the tag in scope for
  // `E`. Printing its `tpe` is what pins that composition against the tag real
  // scalac builds -- a wrong one would still compile and still run.
  def boxTypeImpl[E](c: Context)(implicit e: c.WeakTypeTag[E]): c.Expr[String] = {
    import c.universe._
    c.Expr[String](Literal(Constant(weakTypeOf[ExBox[E]].toString)))
  }
}
