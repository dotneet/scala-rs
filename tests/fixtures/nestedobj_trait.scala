// A member `object` of a *trait*: the interface only declares the accessor,
// and every implementing class carries its own field and body. Also covers a
// nested `case class` (whose companion is nested too) and a `case class` as
// the enclosing template. Expected output is scalac 2.13.16's.
trait Comp {
  val base: Int
  object Opt { def one = base + 1 }
  object Two { def two = Opt.one * 2 }
  class Cell(val n: Int) { def total = base + n }
}

class Impl(val base: Int) extends Comp

case class Holder(k: Int) {
  object Inner { def f = k * 10 }
}

// A `case class` nested in a class: it takes the enclosing instance first,
// and its equality still works. (The path-dependent companion — `bx.Pair(6)`
// and the `copy` that goes through it — is a separate typer gap.)
class Box(val k: Int) {
  case class Pair(a: Int) { def sum = a + k }
}

object Main {
  def main(args: Array[String]): Unit = {
    val i = new Impl(5)
    println(i.Opt.one)
    println(i.Two.two)
    println(new i.Cell(3).total)
    println(i.Opt eq i.Opt)
    val j = new Impl(100)
    println(j.Two.two)
    println(i.Opt eq j.Opt)

    val h = Holder(4)
    println(h.Inner.f)
    val bx = new Box(4)
    val p = new bx.Pair(6)
    println(p.sum)
    println(new bx.Pair(6) == p)

    val anon = new Comp { val base = 20 }
    println(anon.Two.two)
  }
}
