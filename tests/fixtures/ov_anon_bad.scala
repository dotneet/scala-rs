// Rule 9 for an anonymous class: `new T {}` leaves `f` deferred.
//
// scalac 2.13.16: "object creation impossible." / "Missing implementation for
// member of trait T:".
object Main {
  trait T { def f: Int }
  def main(args: Array[String]): Unit = {
    val t = new T {}
    println(t.f)
  }
}
