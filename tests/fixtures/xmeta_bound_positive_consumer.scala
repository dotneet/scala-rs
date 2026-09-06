import xmetaexistential.{Concrete, Provider, TableQuery}

object Main {
  val query: TableQuery[Concrete] = Provider.query
  val row: Concrete = query.value
  def main(args: Array[String]): Unit = println(row.kind)
}
