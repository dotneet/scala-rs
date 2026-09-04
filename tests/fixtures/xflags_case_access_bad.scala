// What `-Xsource-features:case-apply-copy-access` is *for*: without it, the
// synthesized `apply` and `copy` are public and route straight around a
// private constructor. scalac 2.13.16 compiles this file cleanly with no
// flag, and rejects `a`, `b`, `c` and `e` with the feature on.
//
// `d` (`D(1)` for a `protected` constructor) stays legal on purpose: nsc
// copies `protected` onto `copy` but not onto `apply`.
// `f` and `g` stay legal because `Use` is inside `xflags`, which is what
// `private[xflags]` allows.
package xflags

case class C private (x: Int)
case class D protected (x: Int)
case class E private[xflags] (x: Int)

object Use {
  def a: C = C(1)
  def b: C = C.apply(1)
  def c(v: C): C = v.copy(x = 2)
  def d: D = D(1)
  def e(v: D): D = v.copy(x = 2)
  def f: E = E(1)
  def g(v: E): E = v.copy(x = 2)
}
