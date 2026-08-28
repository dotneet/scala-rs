case class MColumn(name: String, columnDef: Option[String])
object MColumn {
  val empty: MColumn = MColumn("", None)
  def make(n: String): MColumn = MColumn(n, None)
}
class User(meta: MColumn) {
  def default: Option[Int] = meta.columnDef.map(s => s.length)
}
object Main {
  def main(args: Array[String]): Unit = {
    println(MColumn.make("a"))
    println(MColumn.empty)
    println(new User(MColumn("b", Some("xy"))).default)
    println(new User(MColumn.empty).default)
    println(MColumn("c", None) == MColumn("c", None))
  }
}
