// The client of tests/fixtures/pfx_binlib/PfxLib.scala, compiled against the
// library's class files: an inner class of a separately compiled class
// through a value prefix (`new c.D`), as a method's result read through that
// value, and as a superclass. (The value's own pickled alias, `new
// c.api.D`, is not yet found by the pickle lookup: `type D is not a member
// of Aliases`; see docs/notes/prefix-types-design.md.)
object Main {
  def main(args: Array[String]): Unit = {
    val c = new pfxlib.C("cc")
    val d = new c.D(1)
    println(d.foo("ab"))
    val e: c.D = c.mk(2)
    println(e.foo("abc"))
    val f: c.D = new c.D(3)
    println(f.foo("x"))
    class Sub extends c.D(4) { override def foo(s: String): Int = super.foo(s) * 10 }
    println(new Sub().foo("xy"))
  }
}
