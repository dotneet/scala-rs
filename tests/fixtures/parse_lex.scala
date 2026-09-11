// Lexical corners the scanner has to get right: interpolation escapes and
// holes, interpolated patterns, SIP-27 trailing commas, `; else`, Unicode
// symbol operators and their precedence, `@@` as a name.

case class C(n: Int) {
  def 𐀀(c: C): C = C(n * c.n) // a letter (supplementary): lowest precedence
  def ☀(c: C): C = C(n * c.n) // a symbol (So): highest precedence
  def ☀=(c: C): C = C(n * c.n) // assignment-operator precedence
  def 🌀(c: C): C = C(n * c.n) // supplementary symbol
  def ↑(c: C): C = C(n - c.n) // Sm
  def *(c: C): C = C(n * c.n)
  def +(c: C): C = C(n + c.n)
}

object Tags {
  type Tagged[U] = { type Tag = U }
  type @@[T, U] = T with Tagged[U]
  def tag[U](i: Int): Int @@ U = i.asInstanceOf[Int @@ U]
}

object Main {
  def classify(line: String): String = line match {
    case s"$a-$b" => s"dash($a,$b)"
    case s"<$_>" => "tag"
    case s"key=${v}" => s"kv($v)"
    case s"tab\t$rest" => s"tab($rest)"
    case s"back\\$rest" => s"back($rest)"
    case s"""tq\t$rest""" => s"tq($rest)"
    case s"""bs\\$rest""" => s"bs($rest)"
    case s"$h:${t @ s"$u:$w"}" => s"nested($h|$t|$u|$w)"
    case _ => "none"
  }

  def sum(
    a: Int,
    b: Int,
  ): Int = a + b

  def pick[
    X,
    Y,
  ](x: X, y: Y): (X, Y) = (x, y)

  def sign(n: Int) = if (n < 0) "neg"; else "non-neg"

  def main(args: Array[String]): Unit = {
    val person = "Alice"
    println(s"$"quoted$" and $$ $person")
    println(f"hi$"$$$"")
    println(raw"\"Hello\", $person")
    println(s"""\"Hello\", $person""")
    println(s"""\\TILT\\""")
    println(raw"""\\TILT\\""")
    println(s"""a\tbA""")
    println(s"${1}$$${2}")

    println(classify("1-2"))
    println(classify("<b>"))
    println(classify("key=9"))
    println(classify("tab\tZ"))
    println(classify("back\\Y"))
    println(classify("tq\tW"))
    println(classify("bs\\Q"))
    println(classify("a:b:c"))
    println(classify("zzz"))
    val s"Hello, $name!" = "Hello, James!"
    println(name)

    println(sum(
      1,
      2,
    ))
    val t = (
      1,
      "x",
    )
    val one: Int = (
      23,
    )
    println(t)
    println(one)
    println(pick[Int, String](1, "b",
    ))
    import scala.collection.{
      immutable,
      mutable,
    }
    println(mutable.ListBuffer(1).toList == immutable.List(1))
    List(1, 2, 3) match {
      case List(a, rest @ _*,
      ) => println(s"$a $rest")
    }
    println(sign(-1) + " " + sign(1))

    val c = C(3)
    val d = C(4)
    println(c ☀ d + d)
    println(c ☀= d + d)
    println(c 𐀀 d + d)
    println(c 🌀d + d)
    println(c ↑ d * d)
    println(Tags.tag[String](42) + 1)
  }
}
