// A stand-in for scala-library, compiled by real scalac and handed to
// scala-rs on `-cp` only. Its shapes are the ones `scala.collection` has.
//
// `Ops.fac` is declared wide, `Wide` implements it wide, `Narrow` overrides it
// covariantly — and nsc emits **no** bridge on the `Narrow` interface, exactly
// as `scala.collection.immutable.IndexedSeq` carries no
// `iterableFactory()Lscala/collection/IterableFactory;`. A class mixing
// `Narrow` in has to carry that bridge itself, or a wide call lands on `Wide`'s
// default: the maximally specific super-interface *for that descriptor*
// (JVMS 5.4.3.3).
//
// `Ops.toString` is the other half: a method inherited from the superclass —
// and every class has `java.lang.Object` above it — always wins over an
// interface default, so the class needs a forwarder to `Ops.toString$`.
package ifb

trait Fac {
  def name: String = "Fac"
}
trait SubFac extends Fac {
  override def name: String = "SubFac"
}

object Facs {
  val plain: Fac = new Fac {}
  val sub: SubFac = new SubFac {}
}

trait Ops[A] {
  def fac: Fac
  def build: String = fac.name
  override def toString: String = "Ops(" + build + ")"
  override def hashCode: Int = build.length
  override def equals(other: Any): Boolean = other.isInstanceOf[Ops[?]]
}

trait Wide[A] extends Ops[A] {
  override def fac: Fac = Facs.plain
}

trait Narrow[A] extends Wide[A] {
  override def fac: SubFac = Facs.sub
}
