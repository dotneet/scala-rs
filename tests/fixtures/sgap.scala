object Main {
  // --- item 1: @inline/@noinline are accepted anywhere, like real scalac ---
  @inline val inlineVal: Int = 41
  @inline @noinline def bothAnnots(): Int = 1

  // --- item 3a: case class with a curried (multi-parameter-list) primary
  // constructor gets a properly curried companion `apply` ---
  case class Pair(a: Int)(val b: Int, val c: Int) {
    def sum: Int = a + b + c
  }

  // --- item 3b: a case class whose primary constructor parameter type is
  // nested inside its own companion object, with the companion declared
  // *after* the case class (forward reference), including a case object
  // extending a parameterized sealed class ---
  final case class Ordering(direction: Ordering.Direction)

  object Ordering {
    sealed abstract class Direction(val desc: Boolean)
    case object Asc extends Direction(false)
    case object Desc extends Direction(true)
  }

  // --- item 3c: Option.flatMap is properly polymorphic (B may differ from A) ---
  case class Box(n: Int) {
    def label: Option[String] = if (n > 0) Some(s"n=$n") else None
  }

  // --- item 4: if/else branches on None/Some (no ascription) get the right
  // lub (Option[X], not Any), so .getOrElse resolves ---
  def firstPositive(empty: Boolean, x: Int): Int = {
    val found = if (empty) None else Some(x)
    found.getOrElse(-1)
  }

  def main(args: Array[String]): Unit = {
    println(inlineVal + bothAnnots())
    val p = Pair(1)(2, 3)
    println(p.sum)
    val o: Ordering = Ordering(Ordering.Desc)
    println(o.direction.desc)
    val box: Option[Box] = Some(Box(5))
    val label = box.flatMap(_.label)
    println(label.getOrElse("none"))
    println(firstPositive(false, 3))
    println(firstPositive(true, 0))
  }
}
