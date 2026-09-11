// Calls that are *not* the singleton their expected type names. scalac
// 2.13.16 rejects every definition below except `ok`.
class A {
  def me: this.type = this
  def m(x: Int)(y: Int): this.type = this
  def add(x: Int): this.type = this
}
class B extends A {
  val other = new B
  def f1: this.type = other.me                      // another path
  def f2: this.type = m(1)                          // a missing argument list
  def f3(b: B): b.type = new B().me                 // not a stable prefix
  def f4(a: A, b: A): b.type = a.me                 // a different parameter
  def ok(a: A): a.type = a.add(1).me.m(1)(2)
  def f6[D <: A](d: D, e: D): e.type = d.add(1)     // a `D`, not `e.type`
}
class Outer {
  def me: this.type = this
  class Inner {
    def f: this.type = me                           // `Outer.this.type`
  }
}
