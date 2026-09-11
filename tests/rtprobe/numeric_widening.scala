// Numeric widening and narrowing, Char/Byte/Short arithmetic (always Int),
// overflow, integer division and remainder signs, shifts, and conversions.
object Main {
  def main(args: Array[String]): Unit = {
    val b: Byte = 127; val s: Short = 32767; val c: Char = 'A'; val i: Int = Int.MaxValue; val l: Long = Long.MaxValue
    println((b + 1) + " " + (b + 1).toByte + " " + (s + 1).toShort + " " + (c + 1) + " " + (c + 1).toChar)
    println((i + 1) + " " + (i + 1L) + " " + (l + 1) + " " + (i * 2) + " " + (i.toLong * 2))
    val w1: Long = i; val w2: Double = l; val w3: Float = i; val w4: Double = 'a'; val w5: Int = 'b'; val w6: Long = c
    println(s"$w1 $w2 $w3 $w4 $w5 $w6")
    println(7 / 2 + " " + -7 / 2 + " " + 7 % -2 + " " + -7 % 2 + " " + 7.0 / 2 + " " + -7.5 % 2)
    println((1 << 31) + " " + (1 << 32) + " " + (1L << 32) + " " + (-8 >> 1) + " " + (-8 >>> 1) + " " + (-8L >>> 1))
    println((3.99).toInt + " " + (-3.99).toInt + " " + 1e20.toInt + " " + 1e20.toLong + " " + Double.NaN.toInt + " " + (-1e20).toInt)
    println(300.toByte + " " + 70000.toShort + " " + 65.toChar + " " + (-1).toChar.toInt + " " + 3000000000L.toInt)
    println(0.1f.toDouble + " " + 16777217.toFloat + " " + (1L << 53).toDouble + " " + ((1L << 53) + 1).toDouble.toLong)
    println(math.abs(Int.MinValue) + " " + math.abs(-5L) + " " + math.max(1, 2L) + " " + math.min(1.5f, 2) + " " + math.round(2.5) + " " + math.round(-2.5) + " " + math.round(2.5f))
    println((5: Int).toHexString + " " + 255.toBinaryString + " " + (-1).toHexString + " " + Integer.MAX_VALUE + " " + java.lang.Long.MIN_VALUE)
    var acc: Byte = 0
    for (_ <- 1 to 200) acc = (acc + 1).toByte
    println(acc)
    var ch = 'a'
    ch = (ch + 2).toChar
    ch = (ch + 1).toChar
    println(ch)
    val sh: Short = (s - 1).toShort
    println(sh * 2)
    println(1 / 3.0f)
    println(10.0 / 3)
    println(10 / 3 * 3.0)
    println(-5.abs + " " + 5.signum + " " + (-5).signum + " " + 5.0.signum + " " + 3.7.floor + " " + 3.2.ceil + " " + (-3.5).round)
    println(Int.MaxValue.toFloat + " " + Long.MaxValue.toFloat + " " + Long.MaxValue.toDouble)
    println((b & 0xff) + " " + ((-1: Byte) & 0xff) + " " + (0xff.toByte) + " " + (c | 0x20).toChar + " " + (c ^ c))
    println(BigInt(2).pow(70) + " " + (BigInt(Long.MaxValue) + BigInt(1)) + " " + BigDecimal("1.10") * BigDecimal(3))
  }
}
