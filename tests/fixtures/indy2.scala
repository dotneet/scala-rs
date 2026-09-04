// The rest of the lambda shapes, against the real scala-library: higher
// arities, a `PartialFunction` (still an anonymous class, as in nsc), a
// user-defined SAM type (also still an anonymous class), a by-name argument,
// a lambda that returns non-locally, and a lambda over an `Array`.
trait Transform {
  def run(s: String): String
}

object Main {
  val mul: (Int, Int) => Int = (a: Int, b: Int) => a * b
  val tri: (Int, Int, Int) => Int = (a: Int, b: Int, c: Int) => a + b + c

  val pf: PartialFunction[Int, String] = {
    case 1 => "one"
    case _ => "many"
  }

  val sam: Transform = (s: String) => s + "?"

  def byName(t: => Int): Int = t + t

  def firstEven(xs: List[Int]): Int = {
    xs.foreach(x => if (x % 2 == 0) return x)
    -1
  }

  def lengths(g: Array[Array[Int]]): List[Int] = g.toList.map(r => r.length)

  def main(args: Array[String]): Unit = {
    println(mul(3, 4))
    println(tri(1, 2, 3))
    println(pf(1))
    println(pf(9))
    println(List(1, 2, 3).collect { case x if x > 1 => x * 10 }.mkString(","))
    println(sam.run("hi"))
    println(byName(5))
    println(firstEven(List(1, 3, 6, 8)))
    println(firstEven(List(1, 3)))
    println(lengths(Array(Array(1, 2), Array(3))).mkString(","))
    println(List(1, 2, 3).map(_ * 2).filter(_ > 2).mkString(","))
  }
}
