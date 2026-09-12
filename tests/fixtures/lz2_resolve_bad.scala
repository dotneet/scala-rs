// agent/libzero2: what the resolution fixes must still refuse.
// Real scalac 2.13.16 rejects every case below.
object Main {
  // 1. A genuinely recursive local `def` with no result type is still a cycle.
  //    (SLS 4.1's forward-reference rule is in `lz2_fwdref_bad`, which has to
  //    be a file of its own: nsc reports it from RefChecks, which does not run
  //    once the typer has reported anything.)
  def bad3(): Int = { def f(n: Int) = if (n == 0) 0 else f(n - 1); f(3) }

  // 2. A polymorphic structural declaration is not satisfied by a member whose
  //    signature differs: `stepper` here returns a bare `S`.
  trait Stp[A]
  trait Eff
  trait Shape[A, S]
  trait Coll[A] { def stepper[S <: Stp[_]](implicit sh: Shape[A, S]): S }
  class Use[A, C](c: C) {
    private type WithEff = Coll[A] { def stepper[S <: Stp[_]](implicit sh: Shape[A, S]): S with Eff }
    def ok(implicit ev: C <:< WithEff): String = "structural"
  }
  class Plain[A] extends Coll[A] { def stepper[S <: Stp[_]](implicit sh: Shape[A, S]): S = ??? }
  def bad4(p: Plain[Int]): String = new Use[Int, Plain[Int]](p).ok

  // 3. An inner class read through its prefix is still conformance-checked.
  trait MySet[A]
  abstract class AbstractSet[A] extends MySet[A]
  trait MapOps[K] { protected class KeySet extends AbstractSet[K] }
  final class HM[K](val n: Int) extends MapOps[K] {
    def wrong: MySet[String] = new KeySet
  }
}
