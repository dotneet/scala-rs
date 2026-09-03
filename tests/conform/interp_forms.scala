// String interpolation in all three flavours, with expressions, formats and
// escapes -- the shape slick's SQL builders use.
object Main {
  case class P(name: String, qty: Int, price: Double)
  def main(args: Array[String]): Unit = {
    val p = P("bolt", 3, 1.5)
    val n = 42
    val xs = List(1, 2, 3)
    println(s"$n items")
    println(s"${p.name} x${p.qty} = ${p.qty * p.price}")
    println(s"nested ${if (n > 10) s"big($n)" else "small"}")
    println(s"escaped $$n and \\ and \"q\"")
    println(s"list: ${xs.mkString(",")} size ${xs.size}")
    println(s"${xs.map(x => s"<$x>").mkString}")
    println(f"$n%d|$n%5d|$n%-5d|")
    println(f"${p.price}%.2f | ${p.price}%08.3f | ${p.price}%+.1f")
    println(f"${p.name}%s|${p.name}%10s|${p.name}%-10s|")
    println(f"${255}%x ${255}%X ${255}%o")
    println(f"${1.0 / 3}%e")
    println(f"${1234567}%,d")
    println(f"100%%")
    println(raw"a\nb\t$n")
    println(raw"""triple ${n}A""")
    println("""multi
line ${n}""")
    println(s"""tri $n ${p.qty + 1}""")
    val sb = new StringBuilder
    sb.append(s"[").append(n).append("]")
    println(sb.toString)
    println(s"${null}")
    println(s"${Some(1)} ${None}")
    println("a" * 3)
    println("%s-%d".format("k", 5))
    println(s"unit ${println("side")}")
    println(f"${'c'}%c${'d'}%c")
    println(f"${true}%b")
    println(s"$n$n")
    val name = "x"
    println(s"$name.$name")
    println(s"${xs.head}%")
  }
}
