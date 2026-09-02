// The same two gaps inside a **macro implementation**, which is where the
// self-built `reify` will need them (`docs/macros.md` §7.13.4).
//
// `reify { e }` expands to
//
//     c.universe.Expr.apply[T](
//       c.universe.rootMirror.asInstanceOf[Mirror[c.universe.type]],
//       new $treecreator1())
//
// and both `c.universe.Expr` and `Mirror[c.universe.type]` were errors before
// this file compiled. The `TreeCreator` below is the other half of that
// shape, written out by hand: its `apply[U]` builds a tree in whatever
// universe the mirror it is handed belongs to.
//
// There is no expansion here -- what is checked is that both compilers accept
// the file and that the class files load with the verifier on.
import scala.reflect.macros.blackbox.Context

object RdCtx {
  def impl(c: Context): c.Tree = {
    import c.universe._

    // The nested object, through the path and through the wildcard import.
    val viaPath = c.universe.Expr
    val viaImport = Expr

    // `c.universe` as a stable identifier in a type argument.
    val m: scala.reflect.api.Mirror[c.universe.type] =
      c.universe.rootMirror.asInstanceOf[scala.reflect.api.Mirror[c.universe.type]]

    // The creator half of the reify shape.
    final class rdcreator1 extends scala.reflect.api.TreeCreator {
      def apply[U <: scala.reflect.api.Universe with Singleton](
          mm: scala.reflect.api.Mirror[U]
      ): U#Tree = {
        val u = mm.universe
        u.Literal(u.Constant(1))
      }
    }
    val creator = new rdcreator1()

    q"(${viaPath.toString.nonEmpty}, ${viaImport.toString.nonEmpty}, ${m.staticClass("scala.Int").fullName}, ${creator.toString.nonEmpty})"
  }
}
