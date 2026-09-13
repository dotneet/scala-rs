// `super.m`: one member set over `this`'s base type sequence, reduced by
// overriding -- nsc's `findMember` on `SuperType(this.thisType,
// intersectionType(parents))`. See `crates/cli/tests/lf.rs`.

// (1) The member is chosen by the LINEARIZATION, not by the last written
// parent clause. `Side` is written last and reaches only `Top.m`; `Mid`
// overrides it with a narrower result, and `C`'s linearization is
// C, Side, Mid, Top -- so `super.m` is `Mid`'s `String`.
// `scala.collection.mutable.ArrayDeque.stepper` is this shape: it mixes in
// `IterableFactoryDefaults` after `IndexedSeqOps`, and only the latter declares
// the `S with EfficientSplit` override.
trait Top { def m: Any = "Top.m" }
trait Mid extends Top { override def m: String = "Mid.m" }
trait Side extends Top
class LinPick extends Mid with Side {
  override def m: String = super.m
}
// ...and the answer is a property of the receiver: swap the clauses and the
// linearization is C, Mid, Side, Top, where `Mid` is still the only one with a
// concrete narrowing `m`, so both orders give `Mid.m`. With *two* overriders
// the order decides; `OrderA`/`OrderB` below are that pair.
trait OverA extends Top { override def m: String = "OverA.m" }
trait OverB extends Top { override def m: String = "OverB.m" }
class OrderA extends OverA with OverB { def pick: String = super.m }
class OrderB extends OverB with OverA { def pick: String = super.m }

// (2) Two POLYMORPHIC sibling overrides of one member, reached through
// different parent clauses. Both write `def f[B](x: B)`, so the two `B`s are
// different symbols and the override reduction has to alpha-convert them before
// it can see one member. Without that, `super.f` was `ambiguous overload`
// (`immutable/Range.scala:153`, `immutable/Vector.scala:196`).
trait PolyBase { def f[B](x: B): String = "PolyBase" }
trait PolyL extends PolyBase { override def f[B](x: B): String = "PolyL" }
trait PolyR extends PolyBase { override def f[B](x: B): String = "PolyR" }
class PolyUse extends PolyL with PolyR {
  def go: String = super.f(1)
}

// (3) An overload whose alternatives live in two UNRELATED mixins is still a
// complete set: returning one clause's members dropped the other's (gitbucket's
// `super.get(path) { … }`, two scalatra traits).
trait GetOne { def g(s: String): String = "one:" + s }
trait GetTwo { def g(s: String, n: Int): String = "two:" + s + n }
class GetBoth extends GetOne with GetTwo {
  def callOne: String = super.g("a")
  def callTwo: String = super.g("a", 2)
}

// (4) `super[P].m` still means exactly `P`'s.
class Qualified extends OverA with OverB {
  def viaA: String = super[OverA].m
  def viaB: String = super[OverB].m
}

// (5) A `super` call whose parent result is the parent's own type parameter:
// the descriptor returns `Object` while this override declares a class, so the
// call needs the cast nsc's erasure inserts. With a `match` above it, the
// missing cast is `VerifyError: Inconsistent stackmap frames` at the merge --
// `scala.collection.immutable.List.appendedAll`'s shape.
trait CcOps[+A, +CC[_]] {
  def mk[B](n: Int): CC[B]
  def app[B >: A](xs: Seq[B]): CC[B] = mk[B](xs.size)
}
class Cc[+A](val n: Int) extends CcOps[A, Cc] {
  def mk[B](k: Int): Cc[B] = new Cc[B](k)
  override def app[B >: A](xs: Seq[B]): Cc[B] = xs match {
    case _ :: _ => new Cc[B](99)
    case _      => super.app(xs)
  }
}

// (6) `super.clone()` in a trait, which is `scala.collection.mutable.Cloneable`:
// `clone` is declared on **AnyRef**, and `linearize` omits AnyRef, so the
// terminal super member has to be resolved through the nearest concrete
// superclass. 57 standard-library classes mix that trait in.
trait Cloneables[+C <: AnyRef] extends java.lang.Cloneable {
  override def clone(): C = super.clone().asInstanceOf[C]
}
class Cell(var v: Int) extends Cloneables[Cell]

object Main {
  def main(args: Array[String]): Unit = {
    println(new LinPick().m)
    println(new OrderA().pick + " " + new OrderB().pick)
    println(new PolyUse().go)
    val gb = new GetBoth
    println(gb.callOne + " " + gb.callTwo)
    val q = new Qualified
    println(q.viaA + " " + q.viaB)
    println(new Cc[Int](0).app(Vector(1, 2)).n + " " + new Cc[Int](0).app(List(1, 2)).n)
    val c = new Cell(7)
    val d = c.clone()
    d.v = 9
    println(c.v.toString + " " + d.v.toString + " " + (c eq d).toString)
  }
}
