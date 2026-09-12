// The other direction: a *blackbox* macro's expansion type must not reach the
// call site. `anyTwo` expands to the very tree `someTwo` does -- `if (true)
// Some(2) else None`, an `Option[Int]` -- but it is declared `Any` behind a
// blackbox `Context`, so real scalac 2.13.16 rejects this file:
//
//   type mismatch;
//    found   : Any
//    required: Option[Int]
//
// Compiled against `rf_wbimpl.scala`.
object Main {
  def main(args: Array[String]): Unit = {
    val x: Option[Int] = RfWb.anyTwo
    println(x)
  }
}
