// Compiled on its own; `ub_sepuse.scala` then reads it back through `-cp`.
// The descriptors say `Lscala/runtime/BoxedUnit;`, so the classfile reader has
// to map that back to `Unit` or our own output does not type-check against
// itself.
//
// The names deliberately start with `L`: a `StackMapTable` frame that named
// `LK` used to come out as `K`, because the descriptor `LLK;` was stripped
// with `trim_start_matches('L')` (which eats *both* leading `L`s).
object Lib {
  def f(x: Unit): String = "libgot"
  def middle(a: Int, b: Unit, c: String): String = c + a
}

case class LK(k: Unit, n: Int)

class LC(val u: Unit) {
  var w: Unit = ()
  def m(x: Unit): String = "m"
}
