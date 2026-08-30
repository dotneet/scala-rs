// Supplying `Enumeration`'s members from its pickle must not make anything
// permissive. Real scalac 2.13.16 rejects every line here too.
object Color extends Enumeration {
  val Red, Green = Value
}
object Main {
  def main(args: Array[String]): Unit = {
    // `withName(s: String)`, not `withName(Int)`
    println(Color.withName(1))
    // no four-argument-shaped `Value`; scalac also rejects this one because
    // `Value` is `protected` and `Main` does not extend `Enumeration`
    println(Color.Value(1, 2))
    // `Enumeration` really does not have this
    println(Color.nosuchMember)
    // a `Value` is not an `Int`, however it is numbered
    val n: Int = Color.Red
  }
}
