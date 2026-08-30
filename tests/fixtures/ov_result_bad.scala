// Rule 1 (SLS 5.1.4): the overriding member's result type must conform to the
// overridden one's. This is the report that started the slice -- scala-rs
// accepted it and the caller's unbox threw `ClassCastException` at run time.
//
// scalac 2.13.16: "incompatible type in overriding".
object Main {
  trait It[A] { def next(): A }
  def main(args: Array[String]): Unit = {
    val i = new It[Int] { def next(): String = "x" }
    println(i.next())
  }
}
