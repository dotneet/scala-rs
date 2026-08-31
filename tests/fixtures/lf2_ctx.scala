// The two `Liftable` instances only a macro implementation can reach: a
// `WeakTypeTag` (which arrives in the implicit clause a macro's type
// parameters come through) and an `Expr` (`c.prefix`).
//
// `docs/macros.md` §7.8. These are the ones slick's `ShapedValue.mapToImpl`
// is blocked on: `q"($rModule.tupled) : ($uTag => $rTag)"` has three holes and
// two of them are tags, so before `Liftable` the whole method reported
//
//     no matching overload for SyntacticFunctionTypeExtractor
//     with arguments (List[TypeTags$WeakTypeTag[U]], TypeTags$WeakTypeTag[R])
//
// Compile-only, and compiled by real scalac 2.13.16 too: *calling* a macro
// needs the JVM bridge (`docs/macros.md` §2.2), which is not built. The trees
// `Liftable` builds are checked against real scalac at run time by
// `lf2_lift.scala`; what this file adds is that the two instances a macro
// implementation needs compile, to a class file that loads and verifies.
import scala.reflect.macros.blackbox

object Lf2Ctx {
  // A tag in term, type and `..$` positions -- `rs.mkTypeTree(tag.tpe)` each
  // time, which is what `Liftable.liftTypeTag` builds.
  def tagged[R, U](c: blackbox.Context)(implicit rTag: c.WeakTypeTag[R], uTag: c.WeakTypeTag[U])
      : c.Tree = {
    import c.universe._
    val inTerm: Tree = q"f($rTag)"
    val inType: Tree = tq"$rTag"
    val fnType: Tree = tq"($uTag) => $rTag"
    val applied: Tree = q"x.asInstanceOf[$rTag]"
    val constructed: Tree = q"new $rTag(1)"
    q"{ ..${List(inTerm, inType, fnType, applied, constructed)} }"
  }

  // `${c.prefix}`, an `Expr[PrefixType]` -- `Liftable.liftExpr` is `_.tree`.
  // slick writes exactly this: `${c.prefix}.toNode`.
  def prefixed(c: blackbox.Context { type PrefixType = List[Int] }): c.Tree = {
    import c.universe._
    q"${c.prefix}.toNode"
  }

  // The shape of slick's `mapToImpl` line 61, all three lifts at once.
  def slickShape[R, U](c: blackbox.Context)(rModule: c.Tree)(implicit
      rTag: c.WeakTypeTag[R],
      uTag: c.WeakTypeTag[U]
  ): c.Tree = {
    import c.universe._
    q"($rModule.tupled) : ($uTag => $rTag)"
  }

  // `symbolOf[T]` / `weakTypeOf[T]`: `def symbolOf[T](implicit tag:
  // WeakTypeTag[T]): TypeSymbol` names `T` only in its implicit clause and
  // nowhere in its result, so `pin_undetermined_tparams` used to refuse the
  // member outright and `symbolOf` was reported as "not found: value"
  // (`docs/macros.md` §7.8). Inside a macro implementation the implicit it
  // asks for is the macro's own tag parameter, which is how slick's
  // `mapToImpl` writes `val rSym = symbolOf[R]`.
  def symbols[R](c: blackbox.Context)(implicit rTag: c.WeakTypeTag[R]): c.Tree = {
    import c.universe._
    val rSym = symbolOf[R]
    val rTpe = weakTypeOf[R]
    q"f($rTpe)"
  }

  // Names and literals, written where slick writes them.
  def named(c: blackbox.Context)(n: c.TermName): c.Tree = {
    import c.universe._
    val s: String = "s"
    val call: Tree = q"h($n, 1, $s)"
    q"{ ..${List(call)} }"
  }
}
