// A macro implementation may ask for a tag whose type argument is an
// *application* of one of its own type parameters. nsc reads which parameter
// the tag is for off `targ.typeSymbol`
// (`Helpers.transformTypeTagEvidenceParams`), which looks through the
// application, so `c.WeakTypeTag[F[Any]]` on `def lift[F[_], G[_]]` is a tag
// for `F` and its fingerprint carries `F`'s index.
//
// cats' only macro is written exactly that way --
// `(implicit evF: c.WeakTypeTag[F[Any]], evG: c.WeakTypeTag[G[Any]])` on
// `FunctionKMacros.lift` -- and reading only the bare-parameter form made the
// whole trailing clause look like ordinary implicit values, so the macro
// *definition* was refused with "macro implementation parameter shape does not
// match the macro definition". Real scalac 2.13.16 accepts this file.
import scala.language.experimental.macros
import scala.reflect.macros.blackbox

trait K2[F[_], G[_]]

object CzeroMacros {
  // The shape that was refused: tags over applied type constructors.
  def liftA[F[_], G[_]](c: blackbox.Context)(
    f: c.Expr[Int]
  )(implicit evF: c.WeakTypeTag[F[Any]], evG: c.WeakTypeTag[G[Any]]): c.Expr[K2[F, G]] = ???

  // The same thing with the tag clause in the other order, which nsc also
  // allows and which the fingerprints have to keep apart.
  def liftB[F[_], G[_]](c: blackbox.Context)(
    f: c.Expr[Int]
  )(implicit evG: c.WeakTypeTag[G[Any]], evF: c.WeakTypeTag[F[Any]]): c.Expr[K2[F, G]] = ???

  // The bare-parameter form, which already worked.
  def liftC[A, B](c: blackbox.Context)(
    f: c.Expr[Int]
  )(implicit evA: c.WeakTypeTag[A], evB: c.WeakTypeTag[B]): c.Expr[K2[List, List]] = ???
}

object CzeroUse {
  def a[F[_], G[_]](f: Int): K2[F, G] = macro CzeroMacros.liftA[F, G]
  def b[F[_], G[_]](f: Int): K2[F, G] = macro CzeroMacros.liftB[F, G]
  def c[A, B](f: Int): K2[List, List] = macro CzeroMacros.liftC[A, B]
}
