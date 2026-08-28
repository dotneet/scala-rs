// `?` as a wildcard type (scalac 2.13 accepts this without -Xsource:3, and
// -Xsource:3 keeps it): `?`, `? <: T` and `? >: T` are aliases for `_`.
trait Level
class Top extends Level

class Shape[L <: Level, T, U](val label: String) {
  def describe: String = label
}

object Main {
  def anyShape(s: Shape[? <: Level, ?, ?]): String = s.describe

  def boundedShape(s: Shape[? <: Level, Int, ?]): String = s.describe

  type SomeShape = Shape[? <: Level, ?, ?]

  def lowerBounded(s: Shape[? >: Top <: Level, ?, ?]): String = s.describe

  // `?` also works with backticks as an ordinary name (here as a term).
  val `?` : Int = 7

  def main(args: Array[String]): Unit = {
    val s = new Shape[Top, Int, String]("shape")
    println(anyShape(s))
    println(boundedShape(s))
    val t: SomeShape = s
    println(t.describe)
    println(lowerBounded(s))
    println(`?`)
  }
}
