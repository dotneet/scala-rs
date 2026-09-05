// Two implicit conversions in scope offer the *same* operator on the same
// receiver, and only one of them can be applied to the arguments written.
//
// nsc's `adaptToArguments` looks for a view whose result has a member
// applicable to the argument list, so this is not an ambiguity: `a &&& b`
// picks the one-argument alternative and `a &&& (b, g)` picks the
// two-argument one. We compared the conversions alone, found no winner and
// reported "value &&& is not a member of Cell".
//
// This is gitbucket's shape: its `implicit class RichColumn(c1: Rep[Boolean])
// { def &&(c2: => Rep[Boolean], guard: => Boolean) }` sits in scope beside
// slick's one-argument `&&`, and every `a && b` in the project was rejected.

class Cell(val b: Boolean)

class OneArg(c: Cell) {
  def &&&(o: Cell): Cell = new Cell(c.b && o.b)
}

class TwoArg(c: Cell) {
  def &&&(o: Cell, guard: Boolean): Cell = if (guard) new Cell(c.b && o.b) else c
}

// A third one whose extra parameter has a default: short of the clause is
// still applicable, so this alternative must stay a candidate for one
// argument as well -- and then the tie with `OneArg` is real. It is kept
// under a different name so the applicable set for `&&&` stays a singleton.
class DefaultArg(c: Cell) {
  def |||(o: Cell, guard: Boolean = true): Cell = if (guard) new Cell(c.b || o.b) else c
}

object Conv {
  implicit def one(c: Cell): OneArg = new OneArg(c)
  implicit def two(c: Cell): TwoArg = new TwoArg(c)
  implicit def three(c: Cell): DefaultArg = new DefaultArg(c)
}

object Main {
  import Conv._

  def main(args: Array[String]): Unit = {
    val t = new Cell(true)
    val f = new Cell(false)
    println((t &&& t).b)
    println((t &&& f).b)
    // Written with the dot: `a op (x, y)` is one tupled argument to this
    // parser, which is a separate gap and not what this fixture is about.
    println(t.&&&(f, true).b)
    println(t.&&&(f, false).b)
    println((f ||| t).b)
    println(f.|||(t, false).b)
  }
}
