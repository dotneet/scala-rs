// The other half of the order-independence pair: the same program as
// `oshadow_java_first.scala` with `java.math.BigDecimal` moved *after* the
// calls to `scala.math.BigDecimal`'s companion.
object Main {
  def main(args: Array[String]): Unit = {
    val d = BigDecimal(2)
    val s = BigDecimal("3.5")
    val j = new java.math.BigDecimal("1")
    println(j)
    println(d)
    println(s)
  }
}
