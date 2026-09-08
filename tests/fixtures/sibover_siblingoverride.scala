// Two sibling *overrides* of one inherited member are ordered by the
// **receiver's linearization**, and by nothing else.
//
// `TreeSet[A]` mixes in `IterableFactoryDefaults` (through `Set`, at
// `CC = Set`) and `SortedSetFactoryDefaults` (at `CC = TreeSet`), and both
// write `override def empty: CC[A @uncheckedVariance]` over the one
// `IterableOps.empty`. Neither trait derives from the other, so an owner test
// cannot order them, and both are definitions, so a declaration/definition
// test does not apply. nsc's `findMember` walks the receiver's base type
// sequence and keeps the first match; `Check::drop_sibling_overrides` is that
// rule. `crates/cli/tests/sibover.rs` recompiles this same source with real
// scalac 2.13.16 and compares the output line for line.
//
// Case 1 is the whole claim in four lines: one pair of traits, two classes
// that differ only in the order they are mixed in, and scalac runs a
// *different* override in each. No property of the pair itself can produce
// that answer.
//
// Case 2 is the library's shape, and it is what pins the *direction*. `Narrow`
// reaches `COps` at `Wide[A]` through `CFac1` and at `Narrow[A]` through
// `CFac2`, and `CFac2` is written last, so SLS 5.1.2 puts it first and `emp`
// is `Narrow[A]`. That answer is read back through an overloaded `show`, so
// the choice is observable at run time and not merely a type the compiler
// keeps to itself. A rule that took the linearization-*last* sibling would
// compile Case 1 unchanged -- the JVM picks the body there, not the typer --
// and would fail `bare` and print `took Wide` here.
//
// Writing those two parents the other way round is *rejected* by scalac
// ("incompatible type in overriding"), so in any program that compiles the
// linearization-first sibling is also the one with the narrowest type. That
// case is in `sibover_siblingoverride_bad.scala`; it is why this file cannot
// tell "first in the linearization" apart from "narrowest type", and why the
// implementation follows nsc and uses the linearization, which is also an
// answer when the two types are equal (Case 1).
//
// Case 3 is the guard. `GA.g` overrides `GBase.g` and `GB.g` is a *new*
// alternative, but the two are siblings, `GBase.g` stands above both, and a
// signature test that lets an abstract `T` match `Int` calls them one member.
// Reducing that pair deletes `g(Int)`, which is how `agent/catstail` took
// slick from 0 errors to 7. `Check::same_member_at` substitutes at the
// receiver before comparing, so `T` is `String` here and the two stay two.
//
// Case 4 is a declaration beside a definition: still the older rule's
// business, and this one must keep its hands off it (`agent/liboverload`
// measured what happens when the hierarchy is allowed to outrank a
// `DEFERRED` member).

import scala.annotation.unchecked.uncheckedVariance

// ---- Case 1: the same two traits, mixed in both orders ---------------------
trait Ops { def emp: Ops }
trait Fac1 extends Ops { override def emp: Ops = { println("Fac1.emp"); this } }
trait Fac2 extends Ops { override def emp: Ops = { println("Fac2.emp"); this } }

class Later extends Fac1 with Fac2 // L: Later, Fac2, Fac1, Ops
class Earlier extends Fac2 with Fac1 // L: Earlier, Fac1, Fac2, Ops

// ---- Case 2: the library's shape, and the direction ------------------------
trait COps[+A, +C] { def emp: C }

trait CFac1[+A, +CC[_]] extends COps[A, CC[A @uncheckedVariance]] {
  def f1: CC[A @uncheckedVariance]
  override def emp: CC[A @uncheckedVariance] = { println("CFac1.emp"); f1 }
}
trait CFac2[+A, +CC[_]] extends COps[A, CC[A @uncheckedVariance]] {
  def f2: CC[A @uncheckedVariance]
  override def emp: CC[A @uncheckedVariance] = { println("CFac2.emp"); f2 }
}

class Wide[+A]

// `CFac2` is written last, so it heads the linearization: `emp` is `Narrow[A]`.
class Narrow[+A] extends Wide[A] with CFac1[A, Wide] with CFac2[A, Narrow] {
  def f1: Wide[A] = new Wide[A]
  def f2: Narrow[A] = new Narrow[A]
  // A bare name, read at this class: only the narrower answer typechecks here.
  def bare: Narrow[A] = emp
}

object Which {
  def show(w: Wide[Int]): String = "took Wide"
  def show(n: Narrow[Int]): String = "took Narrow"
}

// ---- Case 3: a genuine overload the guards must not collapse ---------------
trait GBase[T] { def g(x: T): String = "GBase" }
trait GA[T] extends GBase[T] { override def g(x: T): String = "GA" }
trait GB extends GBase[String] { def g(x: Int): String = "GB" }
class GBoth extends GA[String] with GB

// ---- Case 4: a declaration beside a definition -----------------------------
trait DBase { def d: String }
trait DDecl extends DBase { def d: String }
trait DDefn extends DBase { def d: String = "DDefn.d" }
class DBoth extends DDecl with DDefn

object Main {
  def main(args: Array[String]): Unit = {
    new Later().emp
    new Earlier().emp

    val n = new Narrow[Int]
    n.bare
    println(Which.show(n.emp))

    val b = new GBoth
    println(b.g("s"))
    println(b.g(1))

    println(new DBoth().d)
  }
}
