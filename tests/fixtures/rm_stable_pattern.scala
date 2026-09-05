// Stable identifier patterns (SLS 8.1.5) and the two places they were read as
// variable patterns instead -- which matches everything and answers the first
// case for every input.
//
//   * a name that resolves to a `val` (`case VAL =>`): the backend could not
//     tell a resolved value from a fresh pattern variable, because both are
//     `SymKind::Term`;
//   * a backquoted name (``case `f` =>``), which is stable however it is spelled.
//
// Also here: a compound type pattern has to test *every* parent, and a `for`
// generator whose pattern is refutable owes it a `withFilter`.

object Const { final val VAL = 1; final val VAR = 2 }

sealed trait Opt
case object NoNull extends Opt
case object PrimaryKey extends Opt
case object lower extends Opt

trait TA
trait TB
class OnlyA extends TA
class OnlyB extends TB
class Both extends TA with TB

class Holder(f: String) {
  // `f` is a constructor parameter, not a member: the pattern still compares
  // with it rather than binding a new `f`.
  def matches(x: String) = x match {
    case `f` => true
    case _   => false
  }
}

object Main {
  import Const._

  val TOP = 5

  def viaImport(i: Int) = i match { case VAL => "one" case _ => "default" }
  def viaLocal(i: Int) = i match { case TOP => "top" case _ => "default" }
  def viaSelect(i: Int) = i match { case Const.VAL => "one" case _ => "default" }
  def alt(i: Int) = i match { case VAR | VAL => "hit" case _ => "default" }
  def altBind(i: Int) = i match { case v @ (VAR | VAL) => "hit" + v case _ => "default" }

  def kind(x: Any) = x match {
    case _: TA with TB => "tab"
    case _: TA         => "ta"
    case _: TB         => "tb"
    case _             => "none"
  }

  def main(args: Array[String]): Unit = {
    println(viaImport(1) + " " + viaImport(-1))
    println(viaLocal(5) + " " + viaLocal(-1))
    println(viaSelect(1) + " " + viaSelect(-1))
    println(alt(2) + " " + alt(1) + " " + alt(-1))
    println(altBind(2) + " " + altBind(1) + " " + altBind(-1))

    val h = new Holder("abc")
    println(h.matches("abc").toString + " " + h.matches("bippy"))

    println(kind(new OnlyA) + " " + kind(new OnlyB) + " " + kind(new Both) + " " + kind(1))

    // A refutable generator pattern filters; a *single* identifier is always a
    // definition, even upper case and even backquoted.
    val opts = List(PrimaryKey, NoNull, lower)
    for (o @ NoNull <- opts) println("found " + o)
    for (o @ `lower` <- opts) println("found " + o)
    for ((`lower`, i) <- opts.zipWithIndex) println("found " + i)
    for (X <- List("single")) println(X)
    for (`x` <- List("backquoted")) println(`x`)
  }
}
