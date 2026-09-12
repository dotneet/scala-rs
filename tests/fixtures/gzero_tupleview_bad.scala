// The near misses of `gzero_tupleview`. Accepting a view because its own
// clause is "probably fine" is worse than the error it removes, so each of
// these has to stay rejected -- both compilers refuse all three.
trait Ord2 { def tag: String }
class Co2(val tag: String) extends Ord2
class NoView(val n: Int)

object GzeroViews2 {
  implicit def t2[T1, T2](t: (T1, T2))(implicit e1: T1 => Ord2, e2: T2 => Ord2): Ord2 =
    new Co2("(" + e1(t._1).tag + "," + e2(t._2).tag + ")")
}

object GzeroTupleViewBad {
  import GzeroViews2._
  def need[X](x: X)(implicit ev: X => Ord2): Ord2 = ev(x)

  // No conversion for the second element.
  val a = need((new Co2("a"), new NoView(1)))
  // Nor for the first.
  val b = need((new NoView(2), new Co2("b")))
  // The arity does not match the only tuple rule in scope.
  val c = need((new Co2("c"), new Co2("d"), new Co2("e")))
}
