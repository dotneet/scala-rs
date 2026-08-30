// Rule 9 with more than one missing member: scalac reports the class once and
// lists them, so the diagnostic *count* is one, not two.
//
// scalac 2.13.16: "class D needs to be abstract." / "Missing implementations
// for 2 members of trait T."
object Main {
  trait T { def f: Int; val v: String }
  class D extends T
  def main(args: Array[String]): Unit = println(new D().f)
}
