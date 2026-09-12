// agent/libzero2: the name- and member-resolution roots of the same run.
object Main {
  // 1. `sys/process/Parser.scala`'s `def cur: Int = if (done) …` above
  //    `def done = pos >= line.length`: a block-local `def` with **no result
  //    type** is in scope for the whole block, and its type is inferred on
  //    demand when the forward reference asks for it.
  def tokenize(line: String): String = {
    var pos = 0
    val out = new StringBuilder
    def cur: Int = if (done) -1 else line.charAt(pos)
    def bump() = pos += 1
    def done = pos >= line.length
    while (!done) { out.append(cur.toChar); bump() }
    out.toString
  }
  // ... and the local definition shadows an outer one of the same name even for
  // the statements *above* it, exactly as nsc's namer does: `pick`'s body means
  // the local `outer`, so its inferred result is `Int` and the ascription holds.
  // (A reference from an eager `val`'s initialiser would be SLS 4.1's
  // "forward reference ... extends over definition of value", which both
  // compilers refuse -- see `lz2_resolve_bad`.)
  def outer = "outer"
  def shadowed: String = {
    def pick: Int = outer
    def outer = 3
    pick.toString
  }

  // 2. `immutable/HashMap.scala`'s `final class HashKeySet extends
  //    ImmutableKeySet`: a parent that is an **inner class** is written in its
  //    enclosing class's vocabulary, and only the prefix it was written with
  //    instantiates that.
  trait MySet[A] { def tag: String = "set" }
  abstract class AbstractSet[A] extends MySet[A]
  trait MapOps[K, +V, +CC[_, _], +C] {
    protected trait GenKeySet { this: MySet[K] => def has(k: K): Boolean = true }
    def keySet: MySet[K] = new KeySet
    protected class KeySet extends AbstractSet[K] with GenKeySet
  }
  trait ImmMapOps[K, +V, +CC[X, Y] <: ImmMapOps[X, Y, CC, _], +C] extends MapOps[K, V, CC, C] {
    override def keySet: MySet[K] = new ImmutableKeySet
    protected class ImmutableKeySet extends AbstractSet[K] with GenKeySet
  }
  final class HM[K, +V](val n: Int) extends ImmMapOps[K, V, HM, HM[K, V]] {
    override def keySet: MySet[K] = new HashKeySet
    final class HashKeySet extends ImmutableKeySet {
      private[this] def mk(m: HM[K, _]): MySet[K] = if (m eq HM.this) this else m.keySet
      def other: MySet[K] = mk(new HM[K, V](1))
    }
    def otherKeySet: MySet[K] = new HashKeySet().other
  }

  // 3. `collection/SeqView.scala`'s `new SeqView.Sorted(this, ord)`: an
  //    overload's parameter whose class the argument reaches only through a
  //    *base* type, while the alternative's own parameters are still open.
  type AnyConstr[X] = Any
  trait SeqOps2[+A, +CC[_], +C]
  trait MyView[+A] extends SeqOps2[A, MyView, MyView[A]]
  type SomeSeqOps[+A] = SeqOps2[A, AnyConstr, _]
  class Sorted[A, B >: A] private (underlying: SomeSeqOps[A], len: Int, ord: Ordering[B]) {
    def this(underlying: SomeSeqOps[A], ord: Ordering[B]) = this(underlying, 0, ord)
    def tag: String = "sorted"
  }
  trait V[A] extends MyView[A] {
    def sorted[B >: A](implicit ord: Ordering[B]): String = new Sorted(this, ord).tag
  }

  // 4. `PartialFunction.ElementWiseExtractor.unapplySeq`: a **value** used as an
  //    extractor has its `unapply` read as seen from the receiver, so the bound
  //    name gets the receiver's type argument and not the class's own.
  trait PF[-A, +B] extends (A => B) { def unapply(a: A): Option[B] = Some(apply(a)) }
  def firstOf[A, B](pf: PF[A, B], seq: Seq[A]): Option[Seq[B]] =
    Some(seq.map {
      case pf(b) => b
      case _     => return None
    })

  // 5. `collection/convert/StreamExtensions`'s polymorphic structural
  //    refinements: a declaration with type parameters of its own builds, and
  //    `<:<` against it holds exactly when the member really implements it.
  trait Stp[A]
  trait Eff
  trait Shape[A, S]
  trait Coll[A] { def stepper[S <: Stp[_]](implicit sh: Shape[A, S]): S with Eff }
  class Use[A, C](c: C) {
    private type WithEff = Coll[A] { def stepper[S <: Stp[_]](implicit sh: Shape[A, S]): S with Eff }
    def ok(implicit ev: C <:< WithEff): String = "structural"
  }
  class R[A] extends Coll[A] {
    def stepper[S <: Stp[_]](implicit sh: Shape[A, S]): S with Eff = ???
  }

  def main(args: Array[String]): Unit = {
    println(tokenize("abc"))
    println(shadowed)
    println(new HM[String, Int](0).keySet.tag)
    println(new HM[String, Int](0).otherKeySet.tag)
    println(new V[Int] {}.sorted)
    println(firstOf(new PF[Int, String] { def apply(i: Int) = i.toString }, Seq(1, 2)))
    println(new Use[Int, R[Int]](new R[Int]).ok)
  }
}
