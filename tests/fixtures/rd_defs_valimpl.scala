// The macro-invocation path: `reify { val x = 1; x + 1 }`, expanded by
// scala-rs itself and actually run -- not just printed. Compiled on its own
// so `rd_defs_valuse.scala` can expand against it, the split nsc requires
// (§1.3 of `docs/macros.md`).
import scala.reflect.macros.blackbox.Context

object RdDefsHelper {
  def m1Impl(c: Context): c.Expr[Int] = {
    import c.universe._
    reify { val x = 1; x + 1 }
  }
}
