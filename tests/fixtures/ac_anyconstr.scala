// A type-constructor alias whose body does not mention its parameter.
//
// `scala.collection`'s own `type AnyConstr[X] = Any` is one, and it is why
// `IterableOps[A, CC, C]` is an `IterableOps[A, AnyConstr, _]` for *every*
// `CC`: normalizing the alias eta-expands it to `[X]Any`, and `CC[x] <: Any`
// holds whatever `CC` is. nsc decides this in `isHKSubType` -- normalize both
// constructors and compare the bodies with `sameLength` parameters.
//
// Every line prints which method body ran, so an acceptance that picked the
// *wrong* overload cannot pass: `sel` has an `Ops[Int, AnyConstr, _]` overload
// beside an `Any` one, and before the alias reduces, the abstract-`CC` call
// silently fell through to `Any`.
object Lib {
  type AnyConstr[X] = Any
  // An alias for the alias: normalizing follows the whole chain.
  type StillAny[X] = AnyConstr[X]
  // An alias that *does* mention its parameter. It reduces the same way; the
  // difference is that its body then constrains what conforms.
  type ListOf[X] = List[X]

  trait Ops[+A, +CC[_], +C] { def label: String }

  // `scala.collection.IndexedSeq#stepper`'s shape.
  def anyOps(x: Ops[Int, AnyConstr, _]): String = "anyOps(" + x.label + ")"
  def stillAnyOps(x: Ops[Int, StillAny, _]): String = "stillAnyOps(" + x.label + ")"
  def listOps(x: Ops[Int, ListOf, _]): String = "listOps(" + x.label + ")"

  // Overload selection: `Ops` and `Object` are different after erasure, so
  // these are two real methods and the printed name says which conformance
  // question was answered.
  def sel(x: Ops[Int, AnyConstr, _]): String = "sel:anyconstr"
  def sel(x: Any): String = "sel:any"
}

import Lib._

class Concrete extends Ops[Int, List, String] { def label = "concrete" }

// The abstract constructor is the whole point: `CC` is a type *parameter*, so
// there is nothing to compare it to except the reduced alias.
trait FromSelf[+CC[_]] extends Ops[Int, CC, String] {
  def label = "self"
  def viaSelf: String = anyOps(this)
  def viaSelfChained: String = stillAnyOps(this)
  def selViaSelf: String = sel(this)
}

class SelfImpl extends FromSelf[Vector]

object Main {
  // The same question with `CC` bound by a method's own type parameter.
  def viaParam[CC[_]](x: Ops[Int, CC, String]): String = anyOps(x)
  def selViaParam[CC[_]](x: Ops[Int, CC, String]): String = sel(x)

  // Two spellings of one lambda still compare by their bodies: a class
  // constructor against an alias that names it.
  def concreteToListOf(x: Ops[Int, List, String]): String = listOps(x)

  def main(args: Array[String]): Unit = {
    val c = new Concrete
    val s = new SelfImpl
    println(anyOps(c))
    println(stillAnyOps(c))
    println(listOps(c))
    println(concreteToListOf(c))
    println(s.viaSelf)
    println(s.viaSelfChained)
    println(s.selViaSelf)
    println(viaParam(c))
    println(viaParam(s))
    println(selViaParam(c))
    println(sel(c))
    println(sel("not an Ops"))
  }
}
