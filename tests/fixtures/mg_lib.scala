// The *library* half of the `mg_*` set, compiled by real scalac so that its
// `ScalaSignature` carries what only nsc writes: the `MACRO` flag and the
// `@scala.reflect.macros.internal.macroImpl` annotation naming the
// implementation. `mg_use.scala` and `mg_inspect_bad.scala` are then compiled
// against these class files by scala-rs.
//
// `MgQuery` is `slick.lifted.TableQuery` reduced to what matters for
// `docs/macros.md` §5.1: a class, a companion whose parameterless `apply[E]`
// is a macro, and an implementation that builds `tag => new E(tag)` out of the
// `WeakTypeTag`'s own type. gitbucket's `lazy val Issues = TableQuery[Issues]`
// passes a class the run is *itself compiling* as `E`, which is the case these
// fixtures are about.
//
// `MgName` is the other side of that coin: a macro that does not merely carry
// its type argument but asks what the class *is*. scala-rs cannot answer that
// about a class it is still typing, so the placeholder it hands over has no
// info, and the implementation's verdict must not be repeated to the user.
package mgl

import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context

// `n` is a method rather than a `val` because the point here is the macro, and
// a constructor `val` read from another class file is a separate matter.
class MgTag(n0: Int) {
  def n: Int = n0
}

class MgQuery[E](cons: MgTag => E) {
  def at(k: Int): E = cons(new MgTag(k))
}

object MgQuery {
  /** Taking the constructor function, the way `TableQuery.apply[E](cons)` does. */
  def apply[E](cons: MgTag => E): MgQuery[E] = new MgQuery[E](cons)

  /** The macro alternative: `MgQuery[MgRow]` with no value arguments. */
  def apply[E]: MgQuery[E] = macro MgQueryMacroImpl.apply[E]
}

/** slick's `profile.api`: the alias and the companion's value together, which
  * is how a caller reaches `TableQuery` at all. */
object api {
  type MgQuery[E] = mgl.MgQuery[E]
  val MgQuery = mgl.MgQuery
  type MgTag = mgl.MgTag
}

object MgQueryMacroImpl {
  def apply[E](c: Context)(implicit e: c.WeakTypeTag[E]): c.Expr[MgQuery[E]] = {
    import c.universe._
    val cons = c.Expr[MgTag => E](
      Function(
        List(
          ValDef(
            Modifiers(Flag.PARAM),
            TermName("tag"),
            Ident(typeOf[MgTag].typeSymbol),
            EmptyTree
          )
        ),
        Apply(Select(New(TypeTree(e.tpe)), termNames.CONSTRUCTOR), List(Ident(TermName("tag"))))
      )
    )
    reify { MgQuery.apply[E](cons.splice) }
  }
}

/** A case class that really is on the macro classpath, so an implementation
  * that inspects it gets a true answer. */
case class MgCase(x: Int)

object MgName {
  /** Asks the type argument what it is -- slick's `mapToImpl` opens the same
    * way, with `if (!rSym.isClass || !rSym.asClass.isCaseClass) c.abort(...)`. */
  def of[E]: String = macro MgNameImpl.of[E]
}

object MgNameImpl {
  def of[E](c: Context)(implicit e: c.WeakTypeTag[E]): c.Expr[String] = {
    import c.universe._
    val sym = e.tpe.typeSymbol
    if (!sym.isClass || !sym.asClass.isCaseClass) {
      c.abort(c.enclosingPosition, s"${sym.fullName} must be a case class")
    }
    c.Expr[String](Literal(Constant(sym.fullName)))
  }
}
