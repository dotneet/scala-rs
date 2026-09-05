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

  /** A block that binds a pattern `val` of its own. An ordinary `val` inside
    * a block is reified since the `agent/reifydefs` slice; a pattern `val`
    * -- three definitions after parsing, one `SyntacticPatDef` in nsc's own
    * tree -- still is not. */
  def useBlock(c: Context): c.Expr[Int] = {
    import c.universe._
    reify { { val (k, j) = (1, 2); k + j } }
  }

  /** A type argument with no tag in scope: there is nothing to rebuild `T`
    * from, and guessing would put the wrong type into the expansion. */
  def noTag[T](c: Context)(x: c.Expr[T]): c.Expr[T] = {
    import c.universe._
    reify { RbBadHelper.id[T](x.splice) }
  }
}

object RbBadHelper {
  def id[T](x: T): T = x
}
