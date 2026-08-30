// Rule 9 for an `object`: the instance is what cannot be built.
//
// scalac 2.13.16: "object creation impossible." / "Missing implementation for
// member of trait T:".
object Main {
  trait T { def f: Int }
  object O extends T
  def main(args: Array[String]): Unit = println(O.f)
}
