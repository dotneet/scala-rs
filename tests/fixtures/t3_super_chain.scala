// slick's `BasicProfile` / `RelationalProfile` / `SqlProfile` `computeCapabilities`
// chain, reduced: each trait overrides a no-explicit-type method with
// `super.m op something`, and the traits mixed in between (self-typed
// components with no `m` of their own, `self: Mid => `) must not answer for
// `super.m` through their own self-type -- only through real inheritance.
// Also exercises the `object` side of a chained trait `super` call: a plain
// `class` mixing in the same traits already worked.
trait Base {
  def m: Int = 1
}
trait CompA { self: Mid => }
trait CompB { self: Mid => }
trait Mid extends Base with CompA with CompB {
  override def m = super.m + 10
}
trait Top extends Mid {
  override def m = super.m + 100
}

class ClassImpl extends Top
object ObjectImpl extends Top

object Main {
  def main(args: Array[String]): Unit = {
    println(new ClassImpl().m)
    println(ObjectImpl.m)
  }
}
