// A `Unit` argument to a *super* constructor, to an abstract trait member,
// and to a lifted local `def`. The super-constructor call does not go through
// the ordinary argument path, so it needed the singleton pushed there too:
// `class D extends B((), 5)` emitted `aload_0; iconst_5; invokespecial
// B.<init>(BoxedUnit, I)V` and the verifier rejected it.
class B(val u: Unit, val n: Int) {
  def show: String = "B" + n
}

class D extends B((), 5) {
  override def show: String = "D" + n
}

class Dir(val d: Unit)
case object Asc extends Dir(())

trait TT {
  def take(u: Unit): String
}

class E extends TT {
  def take(u: Unit): String = "E"
}

object Main {
  def main(args: Array[String]): Unit = {
    val d = new D
    println(d.show)
    println(d.u)
    println(d.n)
    println(Asc.d)
    val t: TT = new E
    println(t.take(()))
    def local(u: Unit, n: Int): String = "l" + n
    println(local((), 2))
  }
}
