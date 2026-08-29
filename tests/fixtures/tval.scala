// Runtime representation of a trait's `val` / `var`, and `case object`'s
// synthetic members.

trait Named {
  val label: String = "named"
  def shout: String = "<" + label + ">"
}

trait Counted {
  var count: Int = 0
  def bump(): Unit = { count = count + 1 }
  def twice(): Unit = { count += 2 }
}

// An abstract `var` declared in a trait, implemented by a field in the class.
trait HasSize {
  var size: Int
  def grow(): Unit = { size = size * 2 }
}

class Plain extends Named

class Renamed extends Named {
  override val label = "renamed"
}

class Both extends Named with Counted {
  override val label = "both"
  def describe: String = label + ":" + count
}

class Box extends HasSize with Counted {
  var size: Int = 3
}

// A class overriding one mixed-in `val` while inheriting another.
trait Tagged {
  val tag: String = "tag"
}

class OnlyTag extends Named with Tagged {
  override val tag = "T!"
}

object Config extends Named with Counted {
  override val label = "config"
}

sealed trait Dir
case object Asc extends Dir
case object Desc extends Dir

case class Pair(a: Int, b: String)

object Main {
  def show(n: Named): String = n.label + "/" + n.shout

  def main(args: Array[String]): Unit = {
    val p = new Plain
    println(p.label)
    println(p.shout)
    println(show(p))

    val r = new Renamed
    println(r.label)
    println(r.shout)
    println(show(r))

    val b = new Both
    b.bump()
    b.twice()
    println(b.describe)
    b.count = 10
    println(b.count)
    b.bump()
    println(b.count)

    val box = new Box
    box.grow()
    println(box.size)
    box.size = 7
    box.grow()
    println(box.size)
    box.bump()
    println(box.count)

    val t = new OnlyTag
    println(t.label + " " + t.tag)

    Config.bump()
    println(Config.label + " " + Config.count + " " + Config.shout)

    val d: Dir = Asc
    println(Asc)
    println(Desc)
    println(d)
    println(Asc.productPrefix)
    println(Asc.productArity)
    println(Asc.hashCode)
    println(Desc.hashCode)
    println(Asc == Asc)
    println(Asc.equals(Desc))
    println(d == Asc)
    println(d == Desc)

    val q = Pair(1, "x")
    println(q)
    println(q.productPrefix)
    println(q.productArity)
    println(q == Pair(1, "x"))
  }
}
