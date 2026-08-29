package hl

sealed abstract class HList {
  def size: Int
}

final class HCons[H, T <: HList](val head: H, val tail: T) extends HList {
  def size: Int = 1 + tail.size
}

/** `syntax` binds the *type* name `HNil`; the term `HNil` below still has to
  * be reachable from `HNil.type` inside `object HList`. */
object syntax {
  type HNil = hl.HNil.type
}

object HList {
  import syntax._

  // Forward reference to `object HNil`, with the wildcard import above binding
  // the same name in the type namespace.
  def empty: HNil.type = HNil
  val empties: List[HNil.type] = List(HNil, HNil)
  def qualified: hl.HNil.type = HNil
  def alias(x: HNil): Int = x.size
}

object HNil extends HList {
  def size: Int = 0
}

/** A trait and its companion sharing a name: the singleton type of a nested
  * object is reached through the *module class*, not through the trait. */
sealed trait ColumnOption
object ColumnOption {
  case object AutoInc extends ColumnOption
  case object PrimaryKey extends ColumnOption
}

object Options {
  type Auto = ColumnOption.AutoInc.type
  def pk: ColumnOption.PrimaryKey.type = ColumnOption.PrimaryKey
  def auto: hl.ColumnOption.AutoInc.type = ColumnOption.AutoInc
  def count[A]: Int = 1
  def viaTypeArg: Int = count[ColumnOption.AutoInc.type]
}

object Main {
  def main(args: Array[String]): Unit = {
    println(HList.empty.size)
    println(HList.empties.length)
    println(HList.qualified.size)
    println(HList.alias(HNil))
    val c = new HCons[Int, HNil.type](1, HNil)
    println(c.size)
    println(c.head)
    println(Options.pk)
    println(Options.auto)
    println(Options.viaTypeArg)
  }
}
