object InferBad {
  // A *value* of function type is not a PartialFunction, and the alternative
  // that wants one is not made applicable by being more specific.
  def only(p: PartialFunction[Int, Int]): Int = p(1)
  val h: Int => Int = x => x
  def bad1: Int = only(h)

  // The `[B1 >: B]` bound is a bound, not a suggestion.
  class Cov[A, +B](val b: B)
  def upd[A, B, B1 >: B](t: Cov[A, B], v: B1): Cov[A, B1] = new Cov[A, B1](v)
  def bad2(t: Cov[String, Any]): Cov[String, Null] = upd[String, Any, Null](t, null)

  // An upper bound is checked against an explicit type argument.
  def narrow[A1 <: Char](x: A1): A1 = x
  def bad3: Any = narrow[Int](1)

  // A member that overrides an inherited `implicit def` *without* `implicit`
  // is not an implicit value (scalac 2.13 rejects this too).
  trait Ord[A]
  trait HasOrd[K] { implicit def ordering: Ord[K] }
  def needs[K: Ord, V]: String = "x"
  class Plain[K, V](o: Ord[K]) extends HasOrd[K] {
    def ordering: Ord[K] = o
    def bad4: String = needs
  }

  // An untyped function literal does not become a `java.lang.Appendable` just
  // because that is the only formal on offer.
  def onlyAppendable(a: java.lang.Appendable): Int = 1
  def bad5: Int = onlyAppendable(x => ())
}
