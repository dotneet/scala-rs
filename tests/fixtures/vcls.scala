trait Univ extends Any {
  def describe: String
}

final class Meters(val n: Int) extends AnyVal with Univ {
  def describe = n + "m"
  def plus(o: Meters): Meters = new Meters(n + o.n)
}

final class Name(val s: String) extends AnyVal with Univ {
  def describe = "<" + s + ">"
}

object Main {
  def twice(u: Univ): String = u.describe + u.describe

  // `}` at the end of a line followed by a line starting with `-`: two
  // statements, not a subtraction.
  def fallback: Int = {
    val x = { 1 }
    -1
  }

  // An operator at the *end* of a line still continues the expression.
  def continued: Int = {
    val a = 1 +
      -2
    a
  }

  def main(args: Array[String]): Unit = {
    val m = new Meters(5)
    println(m.describe)
    println(twice(m))
    println(twice(new Name("ada")))

    val u: Univ = m
    println(u.describe)

    val a: Any = m
    println(a.toString)
    println(a.isInstanceOf[Meters])
    a match {
      case x: Meters => println("meters " + x.n)
      case _ => println("other")
    }

    println(m == new Meters(5))
    println(m == new Meters(6))
    println(m.plus(new Meters(3)).describe)
    println(m.asInstanceOf[Meters].n)

    println(fallback)
    println(continued)
  }
}
