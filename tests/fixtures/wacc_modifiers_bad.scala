// Modifier combinations scalac 2.13.16 refuses (nsc `Namers.validate`);
// scala-rs compiled every one of them.
object Test {
  implicit case class IntOps(i: Int)
  class A {
    sealed def f = 0
    private protected def g = 1
  }
  final trait T
  override class O
  class Foo(value: Int) {
    implicit def this(a: String) = this(a.toInt)
  }
  trait Decl { private def pf: Int; final def ff: Int }
  def local = { def h: Int; 1 }
  trait U { type V }
  trait U1 extends U { abstract override type V = Int }
}
