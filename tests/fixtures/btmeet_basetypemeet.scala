// A base class two parents reach at two different instantiations is read at
// the **meet** of them, not at whichever the linearization lists first.
//
// `agent/basetypeseq` made the walk keep the instantiation the most derived
// *reacher* supplies. That is not the same rule, and the difference is this
// file: two parents can reach one base without either standing above the
// other, and then the linearization order decides nothing about which
// instantiation is right. nsc's `BaseTypeSeqs.compoundBaseTypeSeq` keeps every
// variant and resolves the entry with `mergePrefixAndArgs(variants,
// Variance.Contravariant, _)`, which takes the glb at a covariant parameter
// and the lub at a contravariant one.
//
// Each case below calls a method whose type is a parameter of a base trait
// reached twice, and then uses the result in a way only the merged answer
// admits -- so a compiler that takes the first arrival cannot compile this
// file at all, and one that takes the meet produces a program real scalac
// 2.13.16 reproduces exactly (`crates/cli/tests/btmeet.rs` recompiles this
// same source with scalac and compares).
//
// Case 1 is `scala.collection.immutable.Stream`, cut down: it reaches
// `IterableOps` as `IterableOps[A, Stream, Stream[A]]` through
// `LinearSeqOps` and as `IterableOps[A, Iterable, Iterable[A]]` through
// `Iterable`, and SLS 5.1.2 lists `Iterable` first because it is written last.
// `Iter` and `LinOps` here are siblings: neither is the more derived reacher,
// and the covariant `C` has to come out `Str[A]` all the same.
//
// Case 2 is the other half of the same rule. `Sink`'s parameter is
// *contra*variant, so the meet is the **lub** of the arrivals -- `Sink[Animal]`,
// the one that accepts more -- and a rule that simply took the most derived
// arrival would answer `Sink[Dog]` and reject `take(new Animal)`.
//
// Case 3 is the spelling. `IterableFactoryDefaults[+A, +CC[x]] extends
// IterableOps[A, CC, CC[A @uncheckedVariance]]` is how the library writes the
// arrival that carries the annotation, and nsc's `=:=` does not look at it, so
// `Wrapped[A]` and `Wrapped[A @uncheckedVariance]` are one variant and the
// plain spelling is the one kept.

import scala.annotation.unchecked.uncheckedVariance

trait IterOps[+A, +C] {
  def me: C
  def tail: C = me
}
trait Iter[+A] extends IterOps[A, Iter[A]]
trait LinOps[+A, +C] extends IterOps[A, C]

final class Str[+A](val a: A) extends LinOps[A, Str[A]] with Iter[A] {
  def me: Str[A] = this
  def onlyOnStr: String = "Str"
  // `tail` is `IterOps.tail`, and its `C` is `Str[A]` only if `IterOps` is
  // read at the meet of the two arrivals.
  def check: String = tail.onlyOnStr
}

class Animal { def kind: String = "animal" }
class Dog extends Animal { override def kind: String = "dog" }

trait Sink[-T] {
  def take(t: T): String = "sink"
}
trait Wide[-T] extends Sink[T]
trait Narrow extends Sink[Dog]

final class Both extends Wide[Animal] with Narrow

trait FactDef[+A, +CC[x]] extends IterOps[A, CC[A @uncheckedVariance]]

// `Restated` re-declares the `tail` that `IterOps` defines. That is one member
// and not an overload -- `agent/liboverload`'s rule -- but saying so needs
// `IterOps`' `C` at `Restated` to read `Restated[A]` exactly, and one of the
// three arrivals spells it `Restated[A @uncheckedVariance]`. The rule used to
// be reached by trying every path from `Restated` to `IterOps` under a budget;
// it now reads the merged entry, so the annotated spelling losing the merge is
// what keeps this compiling.
trait Restated[+A] extends LinOps[A, Restated[A]] with Iter[A] with FactDef[A, Restated] {
  def tail: Restated[A]
  def onlyOnRestated: String
  def check: String = tail.onlyOnRestated
}

final class Wrapped[+A](val a: A) extends Restated[A] {
  def me: Wrapped[A] = this
  override def tail: Restated[A] = this
  def onlyOnRestated: String = "Restated"
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new Str[Int](1).check)
    println(new Both().take(new Animal))
    println(new Both().take(new Dog).length.toString)
    println(new Wrapped[Int](1).check)
  }
}
