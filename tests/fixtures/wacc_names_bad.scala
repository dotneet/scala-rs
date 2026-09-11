// A name entered twice into one scope (nsc `Namers.enterInScope`);
// scala-rs compiled every one of these.
object Test {
  class A { val x = 1; val x = 2 }
  class P(y: Int) { val y = 1 }
  object O1 { class C; trait C }
  object O2 { object C; val C = 1 }
  object O3 { class D(s: String = ""); val D = 0 }
  class Q { def z = 1; val z = 2 }
  def params[T, T](a: Int)(a: Int) = a
  def block = { val b = 1; val b = 2; def m = 1; def m = 2; b }
  def pat(v: Any) = v match { case (u, u) => 1 }
}
