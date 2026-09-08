// Ties a fixed-arity alternative must *not* win. Real scalac 2.13.16 reports
// `ambiguous reference to overloaded definition` at both calls -- lines 20 and
// 21 -- and the rule this slice adds has to leave them ambiguous.
//
//   * `s`: the fixed-arity formal is `Any`, which does not conform to `Int`,
//     so it is not as specific as the varargs alternative either. Neither
//     side wins; nsc's answer is an error, not the fixed-arity one.
//   * `u`: two varargs lists. Read as argument types both repeated parameters
//     are unwrapped, and then each signature accepts the other's.
package jvarargs

object Bad {
  def s(x: Any): String = "any"
  def s(x: Int*): String = "varargs"

  def u(x: Int, y: Int*): String = "one-plus"
  def u(x: Int*): String = "all"

  def main(args: Array[String]): Unit = {
    println(s(1))
    println(u(1))
  }
}
