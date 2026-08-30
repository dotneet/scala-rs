// Eighth slice of the `type mismatch` family on slick.
//
// Everything here runs on both the private runtime and the real
// scala-library, so it names only `Option`, `Tuple2` and `String`. The
// jar-only cases (empty and splatted repeated parameters, `Map`, `-Xsource:3`
// `xs*`) are in crates/cli/tests/mismatch8.rs.

package mism8util {
  // `private[mism8util]` names an enclosing *package of the definition*.
  // Resolving the name globally found some other `util` and made every use
  // inaccessible.
  class Cell[T](private[mism8util] val slot: T) {
    def pair(other: Cell[T]): String = slot.toString + "/" + other.slot.toString
  }

  class Sym(val name: String)
  // `name` here is a constructor parameter, not a member: `o.name` on another
  // instance means the inherited `val`.
  class Fun(name: String) extends Sym(name) {
    def sameAs(o: Any): Boolean = o match {
      case o: Fun => name == o.name
      case _      => false
    }
  }
}

// An alias declared in an object: the expected type has to be seen through it
// before a polymorphic call's type parameters are solved.
object Names {
  type Slot = Box[Int]
}

class Box[T](val t: T) {
  override def toString: String = "Box"
}
object Box {
  def empty[T]: Box[T] = new Box[T](null.asInstanceOf[T])
}

// A dependent method type: `p.State` is read off the *argument*.
case class UsedFeatures(aggregate: Boolean, distinct: Boolean)

trait Phase {
  type State
  val name: String
}

class AssignUniqueSymbols extends Phase {
  val name = "assignUniqueSymbols"
  type State = UsedFeatures
}

object Phase {
  val assignUniqueSymbols = new AssignUniqueSymbols
}

class CompilerState(f: UsedFeatures) {
  def get[P <: Phase](p: P): Option[p.State] = Some(f.asInstanceOf[p.State])
}

// The expected type of a tuple reaches its components.
class Dumpable
class Node extends Dumpable
trait TermSymbol
class AnonSymbol extends TermSymbol

object Main {
  def wrap[A](a: A): Box[A] = new Box[A](a)

  def pair(s: AnonSymbol): (Dumpable, Box[TermSymbol]) = (new Node, wrap(s))

  def main(args: Array[String]): Unit = {
    val a: Names.Slot = Box.empty
    println(a)

    val p = pair(new AnonSymbol)
    println(p._2.t.isInstanceOf[TermSymbol])

    val st = new CompilerState(UsedFeatures(true, false))
    println(st.get(Phase.assignUniqueSymbols).map(_.aggregate).getOrElse(false))
    println(st.get(Phase.assignUniqueSymbols).map(_.distinct).getOrElse(true))

    val c = new mism8util.Cell[String]("l")
    println(c.pair(new mism8util.Cell[String]("r")))

    val f = new mism8util.Fun("f")
    println(f.sameAs(new mism8util.Fun("f")))
    println(f.sameAs(new mism8util.Fun("g")))
    println(f.name)
  }
}
