// The *library* half of the `tq_m*` pair, compiled by real scalac so that its
// `ScalaSignature` carries what only nsc writes: the `MACRO` flag and the
// `@scala.reflect.macros.internal.macroImpl` annotation naming the
// implementation. `tq_muse.scala` is then compiled against these class files
// by scala-rs, with no source for any of it.
//
// This is `slick.lifted.TableQuery` reduced to what matters: a class, a
// companion with two `apply` alternatives -- one taking the constructor
// function, one a *macro* taking none -- and the implementation that builds
// `tag => new E(tag)` out of the `WeakTypeTag`'s own type with `reify`.
//
// Both halves are in this one file, exactly as slick writes them: nsc requires
// a macro implementation to be compiled before the *expansion*, not before the
// definition, and nothing here expands anything.
package tqm

import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context

class TqTag(val n: Int)

class TqRow(tag: TqTag) {
  def label: String = "row" + tag.n
}

class TqOther(tag: TqTag) {
  def label: String = "other" + tag.n
}

class TqQuery[E](cons: TqTag => E) {
  def head: E = cons(new TqTag(1))
  def at(n: Int): E = cons(new TqTag(n))
}

object TqQuery {
  /** Taking the constructor function, the way `TableQuery.apply[E](cons)` does. */
  def apply[E](cons: TqTag => E): TqQuery[E] = new TqQuery[E](cons)

  /** The macro alternative: `TqQuery[TqRow]` with no value arguments. */
  def apply[E]: TqQuery[E] = macro TqQueryMacroImpl.apply[E]
}

/** slick's `profile.api`: the alias and the companion's value, re-exported
  * together, which is how a caller reaches `TableQuery` at all. */
object api {
  type TqQuery[E] = tqm.TqQuery[E]
  val TqQuery = tqm.TqQuery
  // slick exports `Tag` the same way, and it has to be in scope at the call
  // site: the expansion carries `(tag: TqTag) => …` as a *name*, because the
  // JVM bridge rebuilds a reflect tree from its printed form and a symbol
  // does not survive that (`docs/macros.md` §4.3).
  type TqTag = tqm.TqTag
}

object TqQueryMacroImpl {
  def apply[E](c: Context)(implicit e: c.WeakTypeTag[E]): c.Expr[TqQuery[E]] = {
    import c.universe._
    val cons = c.Expr[TqTag => E](
      Function(
        List(
          ValDef(
            Modifiers(Flag.PARAM),
            TermName("tag"),
            Ident(typeOf[TqTag].typeSymbol),
            EmptyTree
          )
        ),
        Apply(Select(New(TypeTree(e.tpe)), termNames.CONSTRUCTOR), List(Ident(TermName("tag"))))
      )
    )
    reify { TqQuery.apply[E](cons.splice) }
  }
}
