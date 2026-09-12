// What the typed-body reifier still refuses, each named. Real scalac accepts
// all of these (`crates/cli/tests/reify2.rs` pins that too): the refusals
// are confessions, not rules.
import scala.reflect.runtime.universe._

object Main {
  // 1. An assignment to a `var` bound outside the body. nsc boxes the
  //    variable and reifies `free.elem`; scala-rs carries a free term by
  //    value, so a write would be lost.
  def writeVar(): Expr[Int] = {
    var w = 1
    reify { w = 2; w }
  }

  // 2. `this` of a class with type parameters, which nsc carries as a free
  //    term whose type mentions the parameters.
  class G[T](val t: T) {
    val code = reify { t }
  }
}
