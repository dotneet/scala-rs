// Macro implementations whose type tags are *applied type constructors*.
// `docs/macros.md` §7.21.
//
// Until this slice a tag descriptor on the wire was a bare class name, so a
// tag for `ClassTag[R]` or `Map[String, List[Int]]` could not even be
// requested and the implementation was never invoked. Every one of gitbucket's
// 31 `mapTo` call sites stopped there: slick's `ShapedValue.mapToImpl` takes a
// `c.Expr[ClassTag[R]]`, and an `Expr`'s tag is part of the request.
//
// `caseInfoImpl` below is `mapToImpl`'s own opening, kept deliberately
// verbatim: the same `isCaseClass` test, the same walk over `decls` picking
// out the case accessors and reading each one's `typeSignature`. If a tag
// arrived carrying the wrong type -- or carrying a type constructor with no
// arguments -- this would print something else, and the dual run against real
// scalac 2.13.16 would catch it.
//
// Compiled on its own so that `gbm_use.scala` can expand against it, the way
// nsc requires (`crates/cli/tests/gbmapto.rs`).
import scala.reflect.macros.blackbox.Context
import scala.reflect.ClassTag

// A case class that is on the *classpath* by the time `gbm_use.scala` is
// compiled, so the engine's mirror can resolve it by name and the
// implementation can interrogate it for real. Its current-run counterpart --
// which is what gitbucket's row classes are -- is in `gbm_bad.scala`.
case class GbmRow(id: Int, name: String)

object GbmImpl {
  // slick's `ShapedValue.mapToImpl`, as far as it reads its type argument.
  def caseInfoImpl[R](c: Context)(ct: c.Expr[ClassTag[R]])(
      implicit rTag: c.WeakTypeTag[R]): c.Tree = {
    import c.universe._
    val rSym = rTag.tpe.typeSymbol
    if (!rSym.isClass || !rSym.asClass.isCaseClass)
      c.abort(c.enclosingPosition, s"${rSym.fullName} must be a case class")
    val fields = rTag.tpe.decls.collect {
      case m: MethodSymbol if m.isCaseAccessor =>
        m.name.toString + ":" + m.typeSignature.toString
    }
    Literal(Constant(ct.staticType.toString + " " + fields.mkString(",")))
  }

  // Does the tag really carry its arguments, or only print as though it did?
  // `typeConstructor` and `typeArgs` are the two halves taken apart again.
  def shapeImpl[A](c: Context)(implicit aTag: c.WeakTypeTag[A]): c.Tree = {
    import c.universe._
    val t = aTag.tpe
    Literal(Constant(t.toString + " = " + t.typeConstructor.toString +
      "[" + t.typeArgs.mkString(", ") + "]"))
  }

  // A tag is a type, so it answers subtyping questions too. `uTag.tpe <:< …`
  // is what `mapToImpl` asks about `HList`.
  def conformsImpl[A, B](c: Context)(
      implicit aTag: c.WeakTypeTag[A], bTag: c.WeakTypeTag[B]): c.Tree = {
    import c.universe._
    Literal(Constant((aTag.tpe <:< bTag.tpe).toString))
  }
}
