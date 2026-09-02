// The forms `reify` refuses, each by name. `docs/macros.md` §7.14.
//
// nsc reifies a local or a parameter as a *free term* carried inside the
// expansion; scala-rs does not build those. Reifying the bare name instead
// would compile and run, and would mean whatever stood at the call site --
// the precise bug reification exists to prevent -- so every such body is an
// error here.
import scala.reflect.macros.blackbox.Context

object RbBad {
  /** A parameter of the macro implementation, not spliced. */
  def useParam(c: Context)(x: c.Expr[Int]): c.Expr[Int] = {
    import c.universe._
    reify { x.hashCode }
  }

  /** A local of the macro implementation. */
  def useLocal(c: Context): c.Expr[Int] = {
    import c.universe._
    val n = 3
    reify { n }
  }

  /** A type, which needs a reifier of its own. */
  def useType(c: Context): c.Expr[Int] = {
    import c.universe._
    reify { (3: Int) }
  }

  /** A block, whose statements would bind names of their own. */
  def useBlock(c: Context): c.Expr[Int] = {
    import c.universe._
    reify { { val k = 1; k } }
  }
}
