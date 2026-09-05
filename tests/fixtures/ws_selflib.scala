// A self type whose class is read from a **jar** offers its members
// unqualified.
//
// `docs/gitbucket.md`'s "what would remove the most next", entry 1. gitbucket
// writes every slick table mix-in as
// `trait BasicTemplate { self: Table[?] => val userName = column[String](…) }`
// and `column` was `not found: value column`. The wildcard is not what breaks
// it -- `Table[String]` failed identically, and a `Table` written in source
// worked with either spelling. What decides it is where the class comes from:
// a jar class's members are completed one name at a time, on demand, and
// `bind_self_type` copies the self type's member list into the template
// scope, which for such a class is empty. A qualified `this.column` resolved
// all along, because `type_select` completes on demand.
//
// The same thing, with the scala-library jar standing in for slick's:
// `Growable`, `Promise` and `StringBuilder` are all outside the hand-written
// prelude, so their members arrive the same way slick's `Table#column` does.
// Every call below is written unqualified, which is the only spelling that
// was broken.
import scala.collection.mutable.Growable
import scala.concurrent.Promise

trait Adder { self: Growable[String] =>
  def addTwice(s: String): Unit = {
    addOne(s)
    addOne(s)
  }
}

// A named type argument rather than a wildcard, and a second part.
trait Completer { self: Promise[Int] =>
  def ok(v: Int): Boolean = trySuccess(v)
}

trait Wild { self: Growable[?] =>
  def wipe(): Unit = clear()
}

// The three traits above are the test: each one's body is compiled, so the
// unqualified call has to resolve *and* to come out as a call on `this`. They
// are deliberately not mixed into anything here -- `class B extends
// ListBuffer[String] with Adder` is rejected for an unrelated reason
// (`illegal inheritance: self-type B does not conform to Growable[String]`,
// a pickled-parent conformance gap that has nothing to do with this slice).
object Main {
  def main(args: Array[String]): Unit = {
    val p = Promise[Int]()
    println(p.isCompleted)
  }
}
