// SAM literals of traits that are not pure interfaces. nsc expands each into
// an ordinary anonymous class, and mixin gives it what any class mixing in
// the trait gets: the fields of the trait's `val`s and `var`s with their
// accessors and mixin setters, its `lazy val`s and member objects, and the
// `$init$` calls, base traits first, which run the trait bodies' statements.
// An initializer statement may call the SAM method itself, and so read what
// the literal captured.
object Log {
  var lines = List.empty[String]
  def add(s: String): Unit = lines = s :: lines
}

trait Base { Log.add("Base"); val base = 1 }
trait Counter extends Base {
  Log.add("Counter " + base)
  val start = 10
  var count = start
  lazy val label = { Log.add("label"); "c" + start }
  object Parts { val n = 3 }
  def step(x: Int): Int
  def tick(): Int = { count = step(count); count }
}
trait Hello { Log.add("hello " + greet("init")); def greet(s: String): String }
trait Gen[A] { val zero: Option[A] = None; def make(i: Int): A }
abstract class Shaped extends Base { val sides = 4; def area(x: Int): Int }

object Main {
  def main(args: Array[String]): Unit = {
    val k = 5
    val c: Counter = x => x + k
    println(List(c.base, c.start, c.count, c.tick(), c.tick(), c.count))
    c.count = 100
    println(List(c.tick(), c.label, c.label, c.Parts.n))
    val suffix = "!"
    val h: Hello = s => s + suffix
    println(h.greet("x"))
    val g: Gen[String] = i => "g" + i
    println(List(g.zero, g.make(2)))
    val sh: Shaped = x => x * x
    println(List(sh.base, sh.sides, sh.area(3)))
    println(Log.lines.reverse)
  }
}
