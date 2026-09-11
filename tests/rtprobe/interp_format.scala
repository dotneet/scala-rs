// String interpolation (s, f, raw), formatting of every primitive, string
// concatenation with null / chars / numbers, and `+` associativity.
object Main {
  case class P(x: Int)
  def main(args: Array[String]): Unit = {
    val i = 42; val l = 1234567890123L; val d = 3.14159; val f = 2.5f; val c = 'Z'; val b = true
    val s: String = null
    println(s"i=$i l=$l d=$d f=$f c=$c b=$b s=$s")
    println(s"expr=${i + 1} nested=${s"in${i}"} obj=${P(1)} list=${List(1, 2)}")
    println(f"$i%5d|$i%-5d|$i%05d|$l%,d|$d%.2f|$d%10.3f|$f%.1f|$c%c|$b%b|${"str"}%s|$i%x|$i%o")
    println(f"${1.0 / 3}%e ${100.0}%g ${-7}%+d ${0.5}%%")
    println(raw"a\nb\t${i}")
    println("a\tb\\n")
    println(1 + 2 + "3" + 4 + 5)
    println("x" + 'c' + 1 + 2L + 1.5 + 2.5f + true + null + ())
    println('a' + 1)
    println('a' + "b")
    println(("" + 'a' + 'b'))
    println('a'.toString + 'b')
    println(s"$d$f$c")
    println(s"${i}px ${i}_x $$ $$i")
    println("%s and %d and %.3f".format("str", 7, 1.0))
    println("%08.3f|%-8s|%8s".format(-3.5, "left", "right"))
    println(String.format("%s=%d", "k", Int.box(5)))
    println(s"multi\nline")
    val m = Map("a" -> 1)
    println(s"map $m opt ${Some(1)} none ${None} tuple ${(1, "x")} arr-len ${Array(1, 2).length}")
    println(1.0.toString + " " + 1.0f.toString + " " + 100.0.toString + " " + 1e10 + " " + 1e-5 + " " + 123456789.0 + " " + 0.1f)
    println(Double.MaxValue + " " + Float.MinPositiveValue + " " + Long.MinValue + " " + Int.MinValue + " " + (0.0 / 0) + " " + (-1.0 / 0))
    println((-0.0).toString + " " + (0.1 + 0.2) + " " + 1.0e7 + " " + 1.0e6 + " " + 12345.678f)
  }
}
