// `f(xs*)` is the `-Xsource:3` splat. At source level 2.13 it is a postfix
// operator, and `*` is not a member of a sequence.
object Use {
  def f(xs: Int*): Int = xs.length
  val n = f(List(1, 2)*)
}
