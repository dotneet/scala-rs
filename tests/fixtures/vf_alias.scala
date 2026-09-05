// An `object` reached through names an `import` brought in from somewhere
// else. Both halves come from slick's
// `object syntax { type :: [+H, +T <: HList] = HCons[H, T]; val :: = HCons;
// type HNil = heterogeneous.HNil.type }`, imported into `HList.scala`.
//
//   * `val :: = HCons` is an extractor *alias*: `case h :: t` calls
//     `HCons$.unapply`, so the receiver is `HCons$` -- not `syntax$`, which
//     is merely where the name was found. We pushed `syntax$.MODULE$`:
//     `VerifyError: Bad type on operand stack … 'syntax$' is not assignable
//     to 'HCons$'` (`slick.collection.heterogeneous.HList$`).
//   * `type HNil = …HNil.type` is a *type*-namespace binding, and a scope
//     that binds a name only there does not hide a term of that name further
//     out. Taking the alias for a stable-id pattern left `HList$.concat`
//     containing `throw new RuntimeException("cannot load HNil")`.
package vf

sealed abstract class HL
class HC(val h: Any, val t: HL) extends HL
object HC {
  def apply(h: Any, t: HL): HC = new HC(h, t)
  def unapply(c: HC): Some[(Any, HL)] = Some((c.h, c.t))
}
object HN extends HL

object syn {
  // Same spelling as the object, in the *type* namespace only.
  type HN = HN.type
  // The extractor alias.
  val :: = HC
  val Cons = HC
}

object Main {
  import syn._

  def one(l: HL): String = l match {
    case HN         => "nil"
    case h :: t     => "cons " + h
    case _          => "other"
  }

  def two(l: HL): String = (l, l) match {
    case (HN, _)       => "nil2"
    case (Cons(h, _), _) => "cons2 " + h
    case _             => "other2"
  }

  // The alias in a *value* position, and its type alias in type position.
  def three(l: HL): String = {
    val e: HN = HN
    "" + (:: eq HC) + " " + (e eq HN)
  }

  def main(args: Array[String]): Unit = {
    println(one(HN))
    println(one(new HC(1, HN)))
    println(two(HN))
    println(two(new HC(2, HN)))
    println(three(HN))
    println(::(3, HN).h)
  }
}
