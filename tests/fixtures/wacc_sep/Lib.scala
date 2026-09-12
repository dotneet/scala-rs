// Compiled on its own (by scalac or by scala-rs), then `Use.scala` against
// it by the other compiler: default getters must link and dispatch across the
// boundary in both directions.
package waccsep
class A { def g(a: Int = 1): Int = a }
class B extends A { override def g(a: Int = 2): Int = a * 10 }
trait T { def k(s: String = "t"): String = s + "T" }
