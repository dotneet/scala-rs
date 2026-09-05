// A self type whose class is read from a **jar** offers its members
// unqualified.
//
// `docs/gitbucket.md`'s "what would remove the most next", entry 1. gitbucket
// writes every slick table mix-in as
// `trait BasicTemplate { self: Table[?] => val userName = column[String](…) }`
// and `column` was `not found: value column`. The wildcard is not what breaks
// it -- `Table[String]` fails identically below, and a `Table` written in
// source works with either spelling. What decides it is where the class comes
// from: a jar class's members are completed one name at a time, on demand,
// and `bind_self_type` copies the self type's member list into the template
// scope, which for such a class is empty. A qualified `this.dequeue()`
// resolved all along, because `type_select` completes on demand.
//
// `scala.collection.mutable.PriorityQueue` stands in for slick's `Table`:
// it is outside the hand-written prelude, so `dequeue` and `max` arrive the
// same way slick's `Table#column` does. Every call below is written
// unqualified, which is the only spelling that was broken.
//
// The traits are deliberately not mixed into anything: their bodies are what
// is compiled, and a `class B extends PriorityQueue[Int] with Q1` needs an
// `Ordering` and a pickled-parent conformance that have nothing to do with
// this slice.
import scala.collection.mutable.PriorityQueue

trait Q1 { self: PriorityQueue[Int] =>
  def take(): Int = dequeue()
  def biggest: Int = max
}

// A wildcard, which is how gitbucket spells it.
trait Q2 { self: PriorityQueue[?] =>
  def takeAny(): Any = dequeue()
}

// A compound self type: the first part still offers its members.
trait Q3 { self: PriorityQueue[Int] with Serializable =>
  def takeThird(): Int = dequeue()
}

// A *nested* template does not: nsc reaches the enclosing template's self
// type through its context chain, and scala-rs still reports
// `not found: value dequeue` for `trait Q4 { self: PriorityQueue[Int] =>
// trait Inner { def d = dequeue() } }`. Answering it needs the member read at
// the *outer* `this` (`dequeue(): Int`, called on `Q4.this`), not just the
// symbol; see `Typer::expose_from_binary_self_type`.

object Main {
  def main(args: Array[String]): Unit = {
    val q = PriorityQueue(3, 1, 2)
    println(q.dequeue())
  }
}
