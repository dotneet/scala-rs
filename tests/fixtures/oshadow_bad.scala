// Nothing in `BigDecimal`'s companion takes an `Option`, so this must be
// rejected -- and the report must show the whole overload set, not the single
// instance `apply(MathContext)` that reading `java.math.BigDecimal` used to
// leave behind.
object Main {
  def main(args: Array[String]): Unit = {
    val j = new java.math.BigDecimal("1")
    println(j)
    println(BigDecimal(Some(1)))
  }
}
