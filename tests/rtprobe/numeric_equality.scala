// Cooperative equality: `==` between primitives of different types, between
// boxed numbers as Any, with BigInt/BigDecimal, NaN and -0.0, and `##`.
object Main {
  def eqAny(a: Any, b: Any): Boolean = a == b
  def eqRef(a: AnyRef, b: AnyRef): Boolean = a == b
  def main(args: Array[String]): Unit = {
    println(1 == 1L); println(1 == 1.0); println('a' == 97); println(1.0f == 1); println(0.1f == 0.1)
    println(eqAny(1, 1L)); println(eqAny(1, 1.0)); println(eqAny('a', 97)); println(eqAny(1.0f, 1.0)); println(eqAny(1, "1"))
    println(eqAny(1: Byte, 1: Short)); println(eqAny(Int.box(3), Long.box(3L)))
    println((1: Any) == 1.0); println((1L: Any) == (1: Any)); println((2.0: Any) == ('': Any))
    println(Int.box(1).equals(Long.box(1L)))
    println(eqRef(Int.box(5), Double.box(5.0)))
    println(BigInt(10) == 10); println(10 == BigInt(10)); println(eqAny(BigInt(10), 10L)); println(BigDecimal(1.5) == 1.5)
    println(BigDecimal("2.0") == BigDecimal("2.00")); println(BigDecimal(2) == 2)
    val nan = Double.NaN
    println(nan == nan); println(eqAny(nan, nan)); println(Double.box(nan).equals(Double.box(nan)))
    println(0.0 == -0.0); println(eqAny(0.0, -0.0)); println(Double.box(0.0).equals(Double.box(-0.0)))
    println(1.## == 1L.##); println(1.0.## == 1.##); println(1.5.## == 1.5f.##); println("a".## == "a".hashCode)
    println((null: Any).##)
    println(List(1, 2) == List(1L, 2L)); println(List(1, 2) == Vector(1, 2)); println(Set(1, 2) == Set(2, 1))
    println(Map(1 -> "a") == Map(1L -> "a")); println(Array(1) == Array(1));
    println(Some(1) == Some(1L)); println((1, 2) == ((1L, 2.0))); println(Left(1) == Left(1.0))
    val xs: List[Any] = List(1, 1L, 1.0, 1.0f, 'x', "1", BigInt(1))
    println(xs.distinct.size)
    println(xs.map(x => xs.count(_ == x)).mkString(","))
    println(Set[Any](1, 1L, 1.0).size)
    println(Int.MaxValue.toFloat == Int.MaxValue)
    println(eqAny(Long.MaxValue, Long.MaxValue.toDouble))
    println(Long.MaxValue == Long.MaxValue.toDouble)
    val x: Integer = 128; val y: Integer = 128
    println(x == y); println(x eq y)
    val cc: Char = 65; println(cc == 'A' && eqAny(cc, 65.0))
  }
}
