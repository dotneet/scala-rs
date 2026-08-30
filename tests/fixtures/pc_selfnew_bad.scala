// A self type and a `new` name their types the same way an `extends` clause
// does. The self type used to draw "illegal inheritance: self-type G does not
// conform to MissingSelf" -- a complaint about a type that does not exist --
// and `new Missing` came out as "not found: value Missing", the wrong
// namespace. `new Missing {}` (an anonymous class) said nothing at all.
class G { self: MissingSelf => }

trait N { self: Missing3 with Missing4 => }

object Obj

object Main {
  val a = new MissingNew
  val b = new MissingNew2 {}
  val c = new Obj
  def main(args: Array[String]): Unit = println("unreachable")
}
