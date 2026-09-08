// A base class reached through two parents at two different instantiations
// must be read at the *most derived* of them -- SLS 5.1.2's linearization is
// what decides which that is, not the order the parents are written in.
//
// Both hierarchies below are the shapes the scala library is built out of, cut
// down to the part that decides an answer. Each one calls a method whose result
// type is the `C` parameter of a base trait reached twice, and then selects a
// member that only the *derived* class has. So a compiler that picks the wrong
// instantiation does not merely print something different -- it cannot compile
// this file at all, and one that picks the right one produces a program whose
// output real scalac 2.13.16 reproduces exactly (`crates/cli/tests/bts.rs`
// recompiles this same source with scalac and compares).
//
// Case 1 is `scala.collection.immutable.HashMap`: `AbstractMap[K, V]` reaches
// `MapOps` as `MapOps[K, V, Map, Map[K, V]]` while the later mixin reaches it
// as `MapOps[K, V, HashMap, HashMap[K, V]]`. Walking the written parents took
// the first, so `super.updatedWith` came back as `Map[K, V1]`.
//
// Case 2 is `scala.collection.immutable.LazyList`, where the redundant mixin
// is written *after* the parent that already extends it: `LinearSeq[A]` alone
// already gives `LinearSeqOps[A, LinearSeq, LinearSeq[A]]`, and the explicit
// `LinearSeqOps[A, LazyList, LazyList[A]]` is the more derived one.

trait Ops[K, +V, +CC[_, _], +C] {
  def me: C
  def widen(k: K): C = me
}
trait Base[K, +V] extends Ops[K, V, Base, Base[K, V]]
abstract class AbstractBase[K, +V] extends Base[K, V]
trait Strict[K, +V, +CC[_, _], +C] extends Ops[K, V, CC, C]

final class Derived[K, +V](val k: K)
  extends AbstractBase[K, V]
    with Strict[K, V, Derived, Derived[K, V]] {
  def me: Derived[K, V] = this
  def onlyOnDerived: String = "Derived"
  // `widen` is `Ops.widen`, and its `C` is `Derived[K, V]` only if `Ops` is
  // read at the instantiation the later mixin supplies.
  def check: String = widen(k).onlyOnDerived
}

trait SeqOps2[+A, +CC[_], +C] {
  def me2: C
  def rest: C = me2
}
trait LinSeq[+A] extends SeqOps2[A, LinSeq, LinSeq[A]]
abstract class AbstractLin[+A] extends LinSeq[A]

final class Lazily[+A](val a: A)
  extends AbstractLin[A]
    with LinSeq[A]
    with SeqOps2[A, Lazily, Lazily[A]] {
  def me2: Lazily[A] = this
  def onlyOnLazily: String = "Lazily"
  def check: String = rest.onlyOnLazily
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new Derived[Int, String](1).check)
    println(new Lazily[Int](1).check)
  }
}
