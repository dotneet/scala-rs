// The *library* half of the `mt2_*` set, compiled by real scalac so that its
// `ScalaSignature` carries what only nsc writes: the `MACRO` flag and the
// `@scala.reflect.macros.internal.macroImpl` annotation, whose payload is
// wrapped in a `TypeApply` carrying the type arguments written on the
// implementation reference. `docs/macros.md` §7.22.
//
// This is slick's `ShapedValue.mapTo` reduced to what matters:
//
//     class ShapedValue[T, U] { def mapTo[R] = macro ShapedValue.mapToImpl[R, U] }
//
// `R` is `mapTo`'s own type parameter and `U` is the *class's*, so the
// implementation asks for two `WeakTypeTag`s where the call site writes one
// type argument. Lining the two up cannot work and nsc does not try: it reads
// `MacroImplBinding.targs` -- these type arguments -- and resolves each one on
// its own, a macro def's parameter from the call site and anything else
// `asSeenFrom` the prefix.
package mt2

import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context

class Shaped[U](val u: U) {
  def mapTo[R]: String = macro Mt2Impl.pairImpl[R, U]
}

/** The owner's parameter reached through a subclass, so that resolving it is a
  * base type of the receiver rather than the receiver itself. */
class SubShaped[V](v: V) extends Shaped[V](v)

object Plain {
  /** The reference writes the macro def's own parameters in the other order.
    * nsc resolves by position in the *reference*, not by the call site's. */
  def swapped[A, B]: String = macro Mt2Impl.pairImpl[B, A]

  /** A type argument that is neither: written out in full and used as it
    * stands. */
  def withFixed[A]: String = macro Mt2Impl.pairImpl[A, Boolean]

  /** The implementation's tag clause is in the other order from its type
    * parameters. A tag is matched to a written type argument by which type
    * parameter it names, never by its position in the clause. */
  def flipped[A]: String = macro Mt2Impl.flippedImpl[A, Long]
}

object Mt2Impl {
  def pairImpl[R, U](c: Context)(implicit
      rTag: c.WeakTypeTag[R],
      uTag: c.WeakTypeTag[U]
  ): c.Tree = {
    import c.universe._
    Literal(Constant("R=" + rTag.tpe.toString + " U=" + uTag.tpe.toString))
  }

  def flippedImpl[R, U](c: Context)(implicit
      uTag: c.WeakTypeTag[U],
      rTag: c.WeakTypeTag[R]
  ): c.Tree = {
    import c.universe._
    Literal(Constant("R=" + rTag.tpe.toString + " U=" + uTag.tpe.toString))
  }
}

/** The type arguments this bridge refuses, gathered so that
  * `tests/fixtures/mt2_bad.scala` can call them. Real scalac 2.13.16 compiles
  * and runs every one of them; scala-rs refuses each with a reason, because
  * what nsc answers here is not a reading of what was written. */
object Awkward {
  /** nsc takes the *symbol* of `List[R]` and then that symbol's own type, so
    * the implementation is handed `List[A]` -- `A` being `List`'s own type
    * parameter, related to nothing at the call site. */
  def listOf[R]: String = macro Mt2Impl.pairImpl[List[R], Int]
}
