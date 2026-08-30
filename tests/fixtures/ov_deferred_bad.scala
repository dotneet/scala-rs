// Rule 4: a deferred re-declaration *un-implements* the concrete member below
// it in the linearization, so the first concrete subclass has to supply a body
// again. Only an implementation more derived than the declaration counts.
//
// scalac 2.13.16: "class C needs to be abstract." / "No implementation found
// in a subclass for deferred declaration".
object Main {
  class B { def f: Int = 1 }
  abstract class M extends B { override def f: Int }
  class C extends M
  def main(args: Array[String]): Unit = println(new C().f)
}
