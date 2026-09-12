// SLS 7.2 twice over: a conversion whose *own* implicit parameter is a view.
//
// `t2[T1, T2](t: (T1, T2))(implicit e1: T1 => Ord, e2: T2 => Ord)` is the shape
// of slick's `Ordered.tuple2Ordered`, which is what gives
// `sortBy { … => issue.issueId.desc -> commentId }` an `Ordered` for its pair.
// Three things had to hold for it to apply:
//
//   * the conversion's own parameters are solved from the *tuple* argument
//     (`unify_conv_tparam` had no `Type::Tuple` case, so `T1`/`T2` stayed
//     open and its clause was searched with them unsolved);
//   * the eligibility check accepts a clause parameter of function type that
//     only a *conversion* can answer (`e2`, here `otherOrd`), not only one a
//     value search finds (`e1`, here `Predef.$conforms`);
//   * so does the application, both when the witness is reached through
//     another implicit and when the view itself is the one being inserted.
//
// Run by both compilers: the converted values have to be the right ones, in
// the right order, not merely typed.
trait Ord { def tag: String }
class Co(val tag: String) extends Ord
class Other(val n: Int)

object GzeroViews {
  implicit def otherOrd(o: Other): Ord = new Co("other" + o.n)
  implicit def t2[T1, T2](t: (T1, T2))(implicit e1: T1 => Ord, e2: T2 => Ord): Ord =
    new Co("(" + e1(t._1).tag + "," + e2(t._2).tag + ")")
  implicit def t3[T1, T2, T3](
    t: (T1, T2, T3)
  )(implicit e1: T1 => Ord, e2: T2 => Ord, e3: T3 => Ord): Ord =
    new Co("(" + e1(t._1).tag + "," + e2(t._2).tag + "," + e3(t._3).tag + ")")
}

object GzeroTupleView {
  import GzeroViews._
  // An implicit *parameter* of function type: the view request itself.
  def need[X](x: X)(implicit ev: X => Ord): Ord = ev(x)
  // The same request behind a value the search has to find first.
  def both[X](x: X, y: X)(implicit ev: X => Ord): String = ev(x).tag + "|" + ev(y).tag

  def main(args: Array[String]): Unit = {
    println(need(new Co("a")).tag)
    println(need(new Other(1)).tag)
    println(need((new Co("a"), new Other(2))).tag)
    println(need((new Other(3), new Co("b"))).tag)
    println(need((new Co("c"), new Other(4), new Other(5))).tag)
    println(need(((new Co("d"), new Other(6)), new Other(7))).tag)
    println(both((new Co("e"), new Other(8)), (new Co("f"), new Other(9))))
  }
}
