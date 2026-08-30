// nsc gives an op-assignment (`+=`, `-=`, `<<=`, ...) precedence 0 -- below
// every other operator. Ranking `+=` with `+` made `n += i + x` parse as
// `(n += i) + x`, whose left operand is `Unit`; the typer then reached for
// `any2stringadd` and reported
//   no matching overload for (String)String with arguments (Int)
object Main {
  def f(x: Int): Int = x * 10
  def g(x: Int): Int = x + 1
  final class Op(val v: Int) { def plus(o: Int): Int = v + o }

  def main(args: Array[String]): Unit = {
    var n = 0
    val i = 1
    val x = 2

    n += i + x
    println(n)
    n -= i + x
    println(n)
    n = 4
    n *= i + x
    println(n)
    n /= i + x
    println(n)
    n = 7
    n %= i + x
    println(n)

    // Compound right-hand sides of every shape.
    var m = 0
    m += f(i) + g(x)
    println(m)
    m = 0
    m += (i + x) * 3
    println(m)
    m = 0
    m += f(x)
    println(m)
    m = 0
    m += (if (i > 0) 5 else 6) + 1
    println(m)
    m = 0
    m += f(g(i)) - x
    println(m)

    var b = 1
    b <<= i + x
    println(b)
    b |= i + x
    println(b)
    b &= 2 + 1
    println(b)
    b ^= 1 + 1
    println(b)

    var d = 1.5
    d += 1.0 + 2.0
    println(d)

    // `String + Any` is a real `String.+`, not an op-assignment rewrite gone
    // wrong: this must keep compiling.
    var s = "a"
    s += 1
    println(s)
    s += "b" + "c"
    println(s)

    // Precedence 0 is *below* the alphabetic operators too, so the whole
    // `new Op(i) plus x` is the right-hand side.
    var t = 0
    t += new Op(i) plus x
    println(t)
  }
}
