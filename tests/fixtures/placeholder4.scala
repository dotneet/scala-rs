// Pattern-matching anonymous functions (`{ case … }`) and placeholder sections
// (`_ + 1`, `f(_, x)`) take their parameter types from the expected type and
// give the case bodies' lub as the result type — never `Any`.
case class Box[A](a: A)

object Main {
  def two(a: Int, b: Int): Int = a * 10 + b
  def use(f: Int => Int): Int = f(3)
  def use2(f: (Int, Int) => Int): Int = f(3, 4)

  def bump(xs: List[Int]): List[Int] = xs.map { case i => i + 1 }
  def tag(xs: List[Int]): List[String] = xs.map { case i => "n" + i }
  def branch(xs: List[Int]): List[Int] = xs.map { case i => if (i > 0) i else -i }
  def nested(xs: List[Int]): List[Int] = xs.map(i => i match { case 0 => 0; case n => n * 2 })
  def unbox(xs: List[Box[Int]]): List[Int] = xs.map { case Box(v) => v * 3 }
  def keep(xs: List[Int]): List[String] = xs.collect { case i if i > 1 => "y" + i }

  val pf: PartialFunction[Int, String] = { case 1 => "one"; case n => "n" + n }
  val f1: Int => Int = _ + 1
  val f2: (Int, Int) => Int = _ + _
  val f3: Int => Int = two(_, 5)
  val f4: (Int, Int) => Int = two(_, _)
  val f5 = two(_, 7)
  val f6: List[List[Int]] => List[List[Int]] = _.map(_.map(_ + 1))
  val f7: Int => String = "v" + _

  def guarded(s: String): String =
    try { if (s.isEmpty) throw new RuntimeException() else s }
    catch { case _: RuntimeException => "caught" }

  def main(args: Array[String]): Unit = {
    println(bump(List(1, 2)))
    println(tag(List(1, 2)))
    println(branch(List(-1, 2)))
    println(nested(List(0, 3)))
    println(unbox(List(new Box[Int](2), new Box[Int](5))))
    println(keep(List(1, 2, 3)))
    println(pf(1) + "/" + pf(4))
    println(f1(1))
    println(f2(1, 2))
    println(f3(1))
    println(f4(1, 2))
    println(f5(2))
    println(f6(List(List(1, 2), List(3))))
    println(f7(9))
    println(use(_ + 2))
    println(use2(_ + _))
    println(guarded("ok"))
    println(guarded(""))
  }
}
