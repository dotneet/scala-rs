// A `val` whose type an `import` needs is completed during the header pass,
// before any constructor is typed. That completion used to be final and
// silent, so the mismatch below was never reported (scalac 2.13.16:
// "type mismatch", and for `new X` "not enough arguments").
class X(x: Int)
object Imported {
  val a = new X("s")
  import a._
  val b = new X
  import b._
}
