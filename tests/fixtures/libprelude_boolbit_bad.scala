// The other side of the audit: members `scala.Boolean` and the numeric value
// classes genuinely do not declare. Supplying `&`, `|`, `^` and `unary_+` is
// only right if it stops exactly where nsc's own declarations stop.
//
// Every line below is rejected by real scalac 2.13.16.
object Main {
  var p: Boolean = true
  var i: Int = 1
  var d: Double = 1.0

  // `scala.Boolean` has `unary_$bang` and nothing else unary: no `~`, no `-`,
  // and no `+` (the `unary_+` this slice adds is on the seven numerics only).
  def a: Int = p.unary_~
  def b: Int = p.unary_-
  def c: Int = p.unary_+
  // The three bitwise operators take a `Boolean`, not an integral.
  def e: Boolean = p & i
  def f: Boolean = p | i
  def g: Boolean = p ^ i
  // ... and no numeric class takes a `Boolean` on its own bitwise operators.
  def h: Int = i & p
  // Bitwise needs two integrals: `scala.Double` declares no `&`, `|` or `^`.
  def j: Double = d & d
}
