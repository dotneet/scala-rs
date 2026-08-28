// `-Xsource:3` varargs patterns: `case Cast(ch*)` is nsc's rewrite of the
// 2.13 spelling `case Cast(ch @ _*)`. Both spellings, and `_*`, mean the same
// thing under the flag.
object Wrapped {
  def unapplySeq(xs: List[Int]): Option[Seq[Int]] = Some(xs)
}

object Main {
  val One = 1

  def star(xs: List[Int]): String = xs match {
    case List(h, t*) => s"$h/$t"
    case List(h)     => s"$h/only"
    case _           => "-"
  }

  def bind(xs: List[Int]): String = xs match {
    case List(h, t @ _*) => s"$h/$t"
    case _               => "-"
  }

  def anon(xs: List[Int]): String = xs match {
    case List(h, _*) => s"$h"
    case _           => "-"
  }

  def custom(xs: List[Int]): String = xs match {
    case Wrapped(all*) => s"all=$all"
    case _             => "-"
  }

  // nsc binds the name whatever its case: `One*` is `One @ _*`, not a match
  // against the stable id `One`.
  def upper(xs: List[Int]): String = xs match {
    case Wrapped(One*) => s"bound=$One"
    case _             => "-"
  }

  def main(args: Array[String]): Unit = {
    println(star(List(1, 2, 3)))
    println(star(List(1)))
    println(star(Nil))
    println(bind(List(1, 2, 3)))
    println(anon(List(1, 2)))
    println(custom(List(4, 5)))
    println(custom(Nil))
    println(upper(List(7, 8)))
  }
}
