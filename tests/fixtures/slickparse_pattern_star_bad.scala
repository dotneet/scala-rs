// Without `-Xsource:3` the `ch*` spelling is a syntax error, exactly as in
// scalac 2.13.16: "bad simple pattern: use _* to match a sequence".
object Main {
  def star(xs: List[Int]): String = xs match {
    case List(h, t*) => s"$h/$t"
    case _           => "-"
  }
  def main(args: Array[String]): Unit = println(star(List(1, 2)))
}
