// Materialising a tag *inside a macro implementation* (`docs/macros.md`
// §7.10).
//
// This is the shape slick's `ShapedValue.mapToImpl` is written in:
//
//     if (uTag.tpe <:< c.typeOf[slick.collection.heterogeneous.HList]) ...
//
// `c.typeOf[T]` asks for a `c.WeakTypeTag[T]` the same way the runtime
// universe does, except that the universe is `c.universe` and the mirror is
// its `rootMirror`. Expanding the macro needs the JVM bridge, which is not
// built, so what is checked is that both compilers accept this file and that
// the class file it produces loads and verifies.

import scala.reflect.macros.blackbox.Context

trait HL
class Rep

object TtCtx {
  def impl(c: Context)(x: c.Tree): c.Tree = {
    import c.universe._
    // A trait and a class, materialised against the macro context's universe.
    val hl = c.typeOf[HL]
    val rep = c.weakTypeOf[Rep]
    // A tag that comes from the implicit clause rather than the materialiser
    // still wins the search: `symbolOf`/`weakTypeOf` on the *implementation's*
    // own type parameter resolve to `rTag` (`docs/macros.md` §7.8).
    val prim = c.typeOf[Int]
    q"($x, ${hl.toString}, ${rep.toString}, ${prim.toString})"
  }

  def implT[R](c: Context)(x: c.Tree)(implicit rTag: c.WeakTypeTag[R]): c.Tree = {
    import c.universe._
    val fromClause = weakTypeOf[R]
    val materialised = c.typeOf[HL]
    q"($x, ${fromClause.toString}, ${materialised.toString})"
  }
}
