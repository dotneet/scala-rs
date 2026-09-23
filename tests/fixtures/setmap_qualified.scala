// A written wildcard formal on a qualified case-class companion is an actual
// argument prototype, not a provisional wildcard invented by the caller.
abstract class BaseColumnOption[+T]
object ColumnOption {
  case object AutoInc extends BaseColumnOption[Nothing]
  case object PrimaryKey extends BaseColumnOption[Nothing]
}
object RelationalProfile {
  object ColumnOption {
    case class Default[T](value: T) extends BaseColumnOption[T]
    case class Length(n: Int, varying: Boolean) extends BaseColumnOption[Nothing]
  }
}
object SqlProfile {
  object ColumnOption {
    case class SqlType(value: String) extends BaseColumnOption[Nothing]
  }
}
object model {
  final case class Column(options: Set[BaseColumnOption[?]])
}

object Probe {
  def make(dbType: Option[String], autoInc: Boolean, generated: Boolean,
           primaryKey: Boolean, length: Option[Int],
           default: Option[RelationalProfile.ColumnOption.Default[?]]): model.Column =
    model.Column(options = Set() ++
      dbType.map(str => SqlProfile.ColumnOption.SqlType(str)) ++
      (if (autoInc || generated) Some(ColumnOption.AutoInc) else None) ++
      (if (primaryKey) Some(ColumnOption.PrimaryKey) else None) ++
      length.map(RelationalProfile.ColumnOption.Length.apply(_, varying = true)) ++
      (if (!autoInc && !generated) default else None))
}

object Main {
  def main(args: Array[String]): Unit = {
    println(Probe.make(Some("TEXT"), true, false, true, Some(5), None).options.size)
    println(Probe.make(None, false, false, false, None,
      Some(RelationalProfile.ColumnOption.Default(1))).options.size)
  }
}
