object Main {
  final case class Key(value: BigInt)
  def main(args: Array[String]): Unit = {
    val integers = implicitly[Ordering[BigInt]]
    val decimals = implicitly[Ordering[BigDecimal]]
    val keys: Ordering[Key] = Ordering.by(_.value)
    println(integers.lt(BigInt("100000000000000000000"), BigInt("100000000000000000001")))
    println(decimals.lt(BigDecimal("1.01"), BigDecimal("1.02")))
    println(keys.gt(Key(BigInt(3)), Key(BigInt(2))))
  }
}
