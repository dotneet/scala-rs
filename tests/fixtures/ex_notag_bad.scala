// A tag scala-rs cannot compose, named rather than approximated.
// `docs/macros.md` §7.12.
//
// `c.Expr[T](tree)` needs an implicit `WeakTypeTag[T]`, which nothing defines:
// it is materialised. For `ExnBox[E]` that means a `staticClass` for `ExnBox`,
// `appliedType`, and a tag for `E` -- and `E` here has none in scope. nsc goes
// further and builds a tag with a *free type symbol* standing for `E`; scala-rs
// does not, and says so instead of building a tag for the wrong type.
import scala.reflect.macros.blackbox.Context

class ExnBox[E](val row: E)

object ExnImpl {
  def noTagImpl[E](c: Context): c.Expr[ExnBox[E]] = {
    import c.universe._
    c.Expr[ExnBox[E]](Literal(Constant(null)))
  }
}
