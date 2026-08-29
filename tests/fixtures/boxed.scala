object Main {
  def takesInt(x: Int): Int = x + 1

  def main(args: Array[String]): Unit = {
    // `Predef.int2Integer` / `Predef.Integer2int`: the two directions of the
    // boxing view. `java.lang.Integer` is a reference type of its own, so
    // neither direction is a no-op.
    val i: java.lang.Integer = 3
    val back: Int = i
    println(i)
    println(back + 1)
    println(takesInt(i))
    println(i.intValue)
    println(i.compareTo(java.lang.Integer.valueOf(4)))
    println(Predef.int2Integer(5))
    println(Predef.Integer2int(java.lang.Integer.valueOf(6)) + 1)

    // Static members of the wrapper classes.
    println(java.lang.Integer.valueOf(7))
    println(java.lang.Integer.parseInt("42"))
    println(java.lang.Integer.MAX_VALUE)
    println(java.lang.Integer.MIN_VALUE)
    println(java.lang.Long.MAX_VALUE)
    println(java.lang.Character.isDigit('4'))
    println(java.lang.Character.isLetter('4'))
    println(java.lang.Double.parseDouble("2.5"))
    println(java.lang.Boolean.parseBoolean("true"))
    println(java.lang.Integer.toBinaryString(10))

    // The wrappers are ordinary reference types, so they can be type
    // arguments; `scala.Long` could not be one.
    val xs = new java.util.ArrayList[java.lang.Long]
    xs.add(7L)
    xs.add(8)
    println(xs.size)
    println(xs.get(0))
    println(xs.get(1))

    // Boxing into `Any` is unchanged.
    val a: Any = 3
    println(a)
    println(a.isInstanceOf[Int])
    println(a.asInstanceOf[Int] + 1)

    // One wrapper per primitive, and a widening on the way in.
    val c: java.lang.Character = 'x'
    println(c.charValue)
    val z: java.lang.Boolean = true
    println(z.booleanValue)
    val d: java.lang.Double = 0.5
    println(d.doubleValue)
    println(d.isNaN)
    val n: java.lang.Integer = 'c'
    println(n)
    val m: java.lang.Long = 9
    println(m.longValue)

    // The value classes still behave as value classes.
    println(1.max(2))
    println((-3).abs)
    println('9'.isDigit)
    println(1.toString + 2L.toString + 'c'.toString + true.toString)
    val arr = Array(1, 2, 3)
    println(arr(0) + arr(2))
  }
}
