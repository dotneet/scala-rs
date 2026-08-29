// Half of the order-independence pair: `java.math.BigDecimal` is mentioned
// *before* `scala.math.BigDecimal`'s companion. `oshadow_java_last.scala` is
// the same program with the two swapped, and both must compile and print the
// same thing.
object Main {
  def main(args: Array[String]): Unit = {
    val j = new java.math.BigDecimal("1")
    val d = BigDecimal(2)
    val s = BigDecimal("3.5")
    println(j)
    println(d)
    println(s)
  }
}
