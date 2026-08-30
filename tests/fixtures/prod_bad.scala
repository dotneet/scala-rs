// `Product` is a case-class thing, and its accessors keep their signatures.
object Main {
  class Plain(val a: Int)
  case class P(x: Int, y: String)

  def main(args: Array[String]): Unit = {
    // Not a case class: no Product members at all.
    println(new Plain(1).productArity)
    println(new Plain(1).productElement(0))
    // The index is an Int, not a String.
    println(P(1, "h").productElement("0"))
    // Not a Product either.
    val bad: Product = new Plain(1)
    println(bad)
  }
}
