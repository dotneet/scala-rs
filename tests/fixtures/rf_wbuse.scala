// Call sites for the whitebox macros of `rf_wbimpl.scala`. Each `val` has an
// expected type the *declared* result type (`Any`) does not satisfy, so the
// file compiles only if the expansion's own type replaced it.
object Main {
  def main(args: Array[String]): Unit = {
    val x: Option[Int] = RfWb.someTwo
    println(x)
    println(x.map(_ + 1))

    val n: Int = RfWb.five
    println(n + 1)

    val s: String = RfWb.hello
    println(s.length)

    // The blackbox declaration beside them keeps `Any`.
    val any: Any = RfWb.anyTwo
    println(any)
  }
}
