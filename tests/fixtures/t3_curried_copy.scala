// slick's `ast/Node.scala`: `final case class TableNode(schemaName, tableName,
// identity, baseIdentity)(val profileTable: Any)`, whose real uses (e.g.
// `AssignUniqueSymbols.scala`) write `t.copy(identity = x)(t.profileTable)`.
final case class Rec(a: Int, b: String)(val extra: Any) {
  def show: String = s"$a/$b/$extra"
}

object Main {
  def main(args: Array[String]): Unit = {
    val r = Rec(1, "x")("e")
    println(r.show)
    val r2 = r.copy(a = 2)(r.extra)
    println(r2.show)
    val r3 = r.copy(b = "y")("f")
    println(r3.show)
    val r4 = r.copy()(r.extra)
    println(r4.show)
  }
}
