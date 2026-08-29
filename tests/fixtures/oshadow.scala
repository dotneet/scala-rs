// Reading one class must not shrink an overload set that is already there.
//
// `scala.math.BigDecimal` declares an *instance* `apply(MathContext)`, and its
// companion declares the seven `apply` overloads programs actually call. The
// two used to compete: touching `java.math.BigDecimal` made the instance one
// expressible, it landed on the class, and every `BigDecimal(...)` after that
// saw only it. Both orders appear below.
object Main {
  // The JDK class is named here, before anything mentions the companion.
  def fromJava(x: java.math.BigDecimal): BigDecimal = BigDecimal(x)

  def main(args: Array[String]): Unit = {
    val j = new java.math.BigDecimal("12.5")
    println(BigDecimal(2))
    println(BigDecimal(3L))
    println(BigDecimal("4.25"))
    println(BigDecimal(BigInt(6)))
    println(fromJava(j))

    val some: Option[BigDecimal] = Some(BigDecimal(j))
    println(some.getOrElse(BigDecimal(0)))
    val none: Option[BigDecimal] = None
    println(none.getOrElse(BigDecimal(-1)))

    // ... and the other way round: the companion first, the JDK class after.
    println(BigDecimal(7))
    println(new java.math.BigDecimal("8.75"))
    println(BigDecimal(9))
  }
}
