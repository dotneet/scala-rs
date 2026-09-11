// scala-rs rejects: explicit type arguments on a nullary Java static
// method followed by `()`.
object Main {
  def main(args: Array[String]): Unit = {
    println(java.util.Optional.empty[String]().isPresent)
    println(java.util.Collections.emptyList[Integer]().size())
  }
}
