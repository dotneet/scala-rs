// The three shapes that must *not* SAM-convert. Each is rejected by real
// scalac 2.13.16 too -- `crates/cli/tests/samconv.rs` compiles this file with
// both and pairs the rejections.
//
// Widening SAM detection to library types is exactly the kind of change that
// can start accepting these, because the rule that fits cats ("find one
// deferred method") also fits a trait with two, and fits a polymorphic one.

import scala.math.Equiv

trait Two { def a(x: Int): Int; def b(x: Int): Int }
trait Poly { def f[A](a: A): A }

object Bad {
  // Two abstract methods: not a SAM.
  val t: Two = x => x
  // A polymorphic abstract method: nsc's `samOf` requires
  // `sam.typeParams.isEmpty`, so this is not a SAM either.
  val p: Poly = x => x
  // The right SAM, the wrong arity.
  val e: Equiv[Int] = (x: Int) => x > 0
}
