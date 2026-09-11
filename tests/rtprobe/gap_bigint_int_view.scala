// scala-rs rejects: `BigInt(x) + 1` -- int2bigInt and long2bigInt both
// apply, and int2bigInt is the more specific (Int weakly conforms to Long).
object Main {
  def main(args: Array[String]): Unit = {
    println(BigInt(Long.MaxValue) + 1)
    println(BigDecimal("1.10") * 3)
    println(1 + BigInt(2))
  }
}
