// Compiled with -Xsource:3: an unapplied method with parameters is
// eta-expanded wherever a value is required -- as a `val`'s right-hand side,
// as the prefix of a selection (`add.tupled`, gitbucket's
// `RepositoryOptions.apply.tupled`), as an argument whose parameter is no
// function type, and in the branches of an `if`.
case class Opts(a: String, b: Int)
object Main {
  def add(a: Int, b: Int): Int = a + b
  def curried(a: Int)(b: Int): Int = a * b
  def poly[A](a: A): List[A] = List(a)
  def two(a: Int)(implicit b: Int): Int = a + b
  def show[F](f: F): String = if (f.isInstanceOf[Function1[_, _]]) "fn1" else "other"
  def main(args: Array[String]): Unit = {
    val f = add; println(f(1, 2))
    println(add.tupled((3, 4)))
    val c = curried; println(c(2)(5))
    val p = poly[Int]; println(p(7))
    println(add.curried(1)(9))
    println(Opts.apply.tupled(("x", 1)))
    val c1 = curried(3); println(c1(4))
    implicit val ii: Int = 100
    val t = two; println(t(1))
    val s = "abc".charAt; println(s(1))
    println(List(curried).map(g => g(2)(3)))
    println(Option(add).map(g => g(4, 5)))
    println(show(curried(1)))
    val i = if (args.isEmpty) add else add; println(i(6, 7))
    val g = (x: Int) => curried; println(g(0)(2)(3))
  }
}
