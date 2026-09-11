// scala-rs rejects: `Boolean.box` (the name also reaches `java.lang.Boolean`
// and comes back as an overload), and the `ScalaNumericAnyConversions`
// members of the rich wrappers (`5.doubleValue`).
object Main {
  def main(args: Array[String]): Unit = {
    println(Boolean.box(true) + " " + Boolean.unbox(java.lang.Boolean.FALSE))
    println(5.doubleValue + " " + 5.floatValue + " " + 7L.intValue + " " + 'a'.isValidChar)
  }
}
