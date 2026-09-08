// A declaration that restates the definition it inherits is one member.
//
// `scala.collection.LinearSeqOps` re-declares the `tail` that `IterableOps`
// defines, and `Stream` re-declares it again: one member reached through
// three classes, and `IterableOps.tail: C` *is* `Stream[A]` at that prefix.
// Two of `drop_overridden`'s rules used to answer the pair in opposite
// directions -- the declaration/definition rule dropped the deferred member,
// the owner rule dropped the one whose owner is above the other's -- so the
// candidate set eliminated itself and the caller got the whole unreduced set
// back. `tail.foreach(f)` inside `Stream.scala` then reported `value foreach
// is not a member of <overload Iterable[A] | Stream[A] | Stream[A]>`.
//
// `Str` below is that shape: on the pre-fix binary its `tail.flag` is
// `value flag is not a member of <overload Iter[A] | Str[A] | Str[A]>`.
// Every line is checked by what it prints, so a selection that binds the
// wrong alternative cannot pass -- each `tail` here has to keep its own
// class's element, and `sum` walks the chain to the end.

trait IterOps[+A, +C] {
  def tail: C = throw new UnsupportedOperationException("IterOps.tail")
  def depth: Int = 0
}
trait Iter[+A] extends IterOps[A, Iter[A]]

// Re-abstracting is overriding, and this one restates what it overrides.
trait LinOps[+A, +C] extends IterOps[A, C] {
  def tail: C
  override def depth: Int = 1
}

// `LinOps` first, so the linearization reaches `IterOps` through `Iter` and
// reads the inherited `tail` as `Iter[A]`: that is what makes the two rules
// visibly disagree, and it is the arrangement `Stream` has in the library.
trait Str[+A] extends LinOps[A, Str[A]] with Iter[A] {
  def tail: Str[A]
  def head: A
  def isLast: Boolean
  // `tail` must be a `Str[A]` here -- not `Iter[A]`, and not an overload of
  // the three -- or none of the three selections below resolves.
  def second: A = tail.head
  def third: A = tail.tail.head
  def lastOne: A = if (isLast) head else tail.lastOne
}

final class Cons[A](val head: A, tl: () => Str[A], val isLast: Boolean) extends Str[A] {
  override def tail: Str[A] = tl()
}

// The case the rule must leave alone: a declaration and a definition whose
// owners are *unrelated*, brought together by a self type. gitbucket's
// `Profile` / `ProfileProvider`. The definition wins, and the declaration
// must not resurface as a second alternative.
final class Thing(val note: String)
trait Decl { val thing: Thing }
trait Defn { self: Decl => lazy val thing: Thing = new Thing("Defn") }
trait Both extends Defn with Decl {
  def noteOfThing: String = thing.note
}
final class BothImpl extends Both

object Main {
  def main(args: Array[String]): Unit = {
    val s2: Str[String] = new Cons("z", () => throw new UnsupportedOperationException("end"), true)
    val s1: Str[String] = new Cons("y", () => s2, false)
    val s0: Str[String] = new Cons("x", () => s1, false)
    println(s0.head)
    println(s0.second)
    println(s0.third)
    println(s0.lastOne)
    println(s0.tail.second)
    println(s0.depth)
    println(new BothImpl().noteOfThing)
  }
}
