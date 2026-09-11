// Sequence patterns: `::`, `+:`, `:+`, `_*`, vector and array patterns,
// nested list patterns, and patterns in val definitions.
object Main {
  def lst(xs: List[Int]): String = xs match {
    case Nil => "empty"
    case x :: Nil => s"one $x"
    case x :: y :: Nil => s"two $x $y"
    case x :: (rest @ (_ :: _)) if x > 100 => s"big head $x then ${rest.size}"
    case x :: y :: rest => s"many $x $y +${rest.size}"
  }
  def seqp(xs: Seq[Int]): String = xs match {
    case Seq() => "Seq()"
    case Seq(1, rest @ _*) => "starts with 1, rest " + rest
    case init :+ last => s"init $init last $last"
  }
  def vec(v: Vector[String]): String = v match {
    case h +: t => s"h=$h t=$t"
    case _ => "empty vector"
  }
  def arr(a: Array[Int]): String = a match {
    case Array() => "empty array"
    case Array(x) => "single " + x
    case Array(x, _*) => "first " + x + " of " + a.length
  }
  def nested(xss: List[List[Int]]): Int = xss match {
    case List(List(a, b), List(c)) => a + b + c
    case (h :: _) :: _ => h
    case _ => 0
  }

  def main(args: Array[String]): Unit = {
    List(Nil, List(1), List(1, 2), List(101, 2, 3), List(1, 2, 3, 4)).foreach(x => println(lst(x)))
    List(Seq(), Seq(1, 2, 3), Seq(5, 6, 7)).foreach(x => println(seqp(x)))
    println(vec(Vector("a", "b", "c"))); println(vec(Vector()))
    println(arr(Array())); println(arr(Array(4))); println(arr(Array(5, 6, 7)))
    println(nested(List(List(1, 2), List(3)))); println(nested(List(List(9, 9, 9)))); println(nested(Nil))
    val a :: b :: _ = List(10, 20, 30)
    println(a + b)
    val (p, q) = (1, "one")
    println(p + q)
    val Seq(s1, s2 @ _*) = Seq("x", "y", "z")
    println(s1 + s2.mkString)
    val Some((k, v)) = Map(1 -> "a").headOption
    println(s"$k -> $v")
    val Date = """(\d+)-(\d+)""".r
    val Date(y, m) = "2024-05"
    println(s"$y/$m")
  }
}
