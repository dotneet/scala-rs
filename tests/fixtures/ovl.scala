// Overload resolution and method application: alias type members, a factory
// companion on a plain class, an overload whose alternatives differ by an
// implicit clause, and a `val` alternative that wins over an inherited method.

class Box[A](val value: A) {
  def get: A = value
}

// An alias member is the same type as its right-hand side, in both directions.
// `Uses` is typed before `Later`, so the alias has to be completed on demand.
object Uses {
  def unwrap(s: Later.Scope): Int = s.get
  def name(n: Cfg.Names): String = n.get
}

object Later {
  type Scope = Box[Int]
  def one: Scope = new Box(1)
}

// A trait and its companion share a name; `Cfg.Names` names the *object*'s
// member, since the prefix of a `p.T` type is a term.
trait Cfg
object Cfg {
  type Names = Box[String]
}

trait Tagged[T] {
  def label: String
}
object Tagged {
  implicit val intTagged: Tagged[Int] = new Tagged[Int] { def label = "int" }
  implicit val stringTagged: Tagged[String] = new Tagged[String] {
    def label = "string"
  }
}

// A plain class whose companion declares `apply` itself: one alternative has a
// default argument, the other a type parameter and a trailing implicit clause.
class Lit(val tpe: String, val value: Any, val volatileHint: Boolean) {
  override def toString: String = tpe + "/" + value + "/" + volatileHint
}
object Lit {
  def apply(tpe: String, value: Any, volatileHint: Boolean = false): Lit =
    new Lit(tpe, value, volatileHint)
  def apply[T](value: T)(implicit t: Tagged[T]): Lit =
    new Lit(t.label, value, false)
}

// A repeated parameter followed by an implicit clause.
object Fn {
  def column[T](ch: Int*)(implicit t: Tagged[T]): String =
    t.label + ":" + ch.length
}

class Op(val name: String) {
  def unapply(a: Node): Option[Int] = if (a.op eq this) Some(a.arg) else None
}
class Node(val op: Op, val arg: Int)

// `val ==` and the inherited `Any.==(x: Any)` are two alternatives; in value
// position only the parameterless one survives, so `Library.==` is the `Op`.
object Library {
  val Not = new Op("not")
  val == = new Op("=")
}

object Main {
  def describe(n: Node): String = n match {
    case Library.Not(v) => "not " + v
    case Library.==(v)  => "eq " + v
    case _              => "other"
  }

  def main(args: Array[String]): Unit = {
    println(Uses.unwrap(new Box(7)))
    println(Uses.unwrap(Later.one))
    println(Uses.name(new Box("cfg")))

    println(Lit("t", 2, true))
    println(Lit("t", 2))
    println(Lit(5))
    println(Lit("s"))

    println(Fn.column[Int](1, 2, 3))
    println(Fn.column[String]())

    println(Library.==.name)
    println(describe(new Node(Library.==, 42)))
    println(describe(new Node(Library.Not, 7)))
  }
}
