import xmetaexistential.{Concrete, Provider, ShapedValue, TableQuery}

object Main {
  val query: TableQuery[Concrete] = Provider.query
  val shaped: ShapedValue[Seq[Concrete], String] = query.shaped
  def main(args: Array[String]): Unit = println(shaped)
}
