// A call that omits a defaulted parameter is typed a *second* time: this
// compiler's `name$default$n` getter takes the parameters that precede the
// default, so the arguments already given are handed to it and re-typed. A
// by-name argument has by then been wrapped into its `Function0` thunk, which
// overload resolution scored as `() => <notype>` and matched to nothing --
// `no matching overload for (=> Option[Node])Option[Node] with arguments
// (() => <notype>)` on slick's `w2.orElse(where)`.

final case class Comp(a: Int,
                      where: Option[Int] = None,
                      having: Option[Int] = None,
                      locking: Option[Int] = None) {
  // `copy` with `locking` omitted, and by-name (`orElse`) arguments
  def merge(w2: Option[Int], h2: Option[Int]): Comp =
    copy(a = a, where = w2.orElse(where), having = h2.orElse(having))
}

object Main {
  val other: Option[Int] = Some(7)
  def f(w: Option[Int], l: Option[Int] = None): String = s"$w $l"

  def main(args: Array[String]): Unit = {
    println(Comp(1, Some(1)).merge(None, Some(2)))
    println(f(other.orElse(other), None)) // nothing omitted: always worked
    println(f(other.orElse(other))) // `l` defaulted away
    println(f(other.orElse(Some(9)))) // ditto, with a different thunk body
  }
}
