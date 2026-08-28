package cake.relational

/** Cake components: their inner classes are reachable only through the
  * self-type, which names a trait declared in another file. Both components
  * call their self alias `self`, and both see the other's through the
  * self-type — the alias is still one name, not an overload. */
trait RelationalTableComponent { self: RelationalProfile =>
  def tableProvider: String = self.profileName

  abstract class Table[T](val tableName: String)
}

trait RelationalSequenceComponent { self: RelationalProfile =>
  class Sequence[T](val seqName: String)
}

object TableSupport {
  trait MultipleRows {
    def rowsPerStatement: Int = 100
  }
}

trait Ref[T] {
  def label: String
}

object Ref {
  abstract class Typed[T](val label: String) extends Ref[T]
}
