// A tag over a type parameter with no tag in scope, built with a *free type*
// (`docs/notes/reify-design.md`).
//
// `c.Expr[T](tree)` needs an implicit `WeakTypeTag[T]`, which nothing defines:
// it is materialised. For `ExnBox[E]` that means a class symbol for `ExnBox`,
// `appliedType`, and a tag for `E` -- and `E` here has none in scope. nsc
// builds a tag with a free type symbol standing for `E`, and so does scala-rs
// now: `newFreeType("E", FlagsRepr(PARAM | DEFERRED), "defined by noTagImpl
// in ex_notag.scala:...")`, which is what the toolbox later reports when it
// is asked to evaluate a tree whose type is still free.
import scala.reflect.macros.blackbox.Context

class ExnBox[E](val row: E)

object ExnImpl {
  def noTagImpl[E](c: Context): c.Expr[ExnBox[E]] = {
    import c.universe._
    c.Expr[ExnBox[E]](Literal(Constant(null)))
  }
}
