// A library compiled separately (by scalac or by scala-rs): an inner class of
// a class, whose constructor carries the hidden enclosing-instance slot in
// the class file, and an alias to it behind a value, as slick's
// `JdbcProfile#API` does (`type Table[T] = RelationalProfile.this.Table[T]`).
package pfxlib

class C(val tag: String) {
  class D(val n: Int) { def foo(s: String): Int = s.length + tag.length + n }
  trait Aliases { type D = C.this.D }
  val api: Aliases = new Aliases {}
  def mk(n: Int): D = new D(n)
}
