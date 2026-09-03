// What is refused around a refined `Context` and around `..$` splices, each
// named.
//
// Real scalac 2.13.16 rejects the first and the third too ("Can't unquote
// with ... here" and "macro implementation has incompatible shape"), so those
// two pin *agreement*: the mixed-splice concatenation must not start
// accepting a rank-2 hole, and reading a `Context` through a refinement must
// not start accepting a refinement of something that is not one. The second
// is scala-rs's own gap -- nsc reifies it -- and is a confession, not a rule.
import scala.language.experimental.macros
import scala.reflect.macros.blackbox.Context

object SvBad {
  /** Rank 2: `...$xss` stands for a list of *clauses*, not a list of trees.
    * The mixed-splice concatenation joins `List[Tree]`s, and a list of lists
    * is not one. */
  def clauses(c: Context): c.Tree = {
    import c.universe._
    val argss = List(List(q"1"), List(q"2"))
    q"f(a, ...$argss)"
  }

  /** A `case` class whose parents are a splice: nsc's parser supplies
    * `Product with Serializable` for a `case` class, and there is no honest
    * place to put them beside a spliced list. */
  def caseParents(c: Context): c.Tree = {
    import c.universe._
    val parents = List(tq"_root_.scala.AnyRef")
    q"case class SvC() extends ..$parents"
  }

  /** A refinement is read for *which* `Context` it refines. A refinement of
    * something else is still not a macro context. */
  def notAContext(c: AnyRef { type PrefixType = Int }): Int = 1
}

object SvBadUse {
  def notAContext: Int = macro SvBad.notAContext
}
