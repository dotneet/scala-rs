import slick.ast.{Ordering => SqlOrdering, ScalaBaseType, ScalaType}
object Main {
  def main(args: Array[String]): Unit = {
    val base: ScalaType[Int] = ScalaBaseType.intType
    val asc = SqlOrdering().asc.nullsFirst
    val desc = SqlOrdering().desc.nullsLast
    println(base.scalaOrderingFor(asc).compare(1, 2))
    println(base.scalaOrderingFor(desc).compare(1, 2))
    val opt = base.optionType
    println(opt.scalaOrderingFor(asc).compare(None, Some(1)))
    println(opt.scalaOrderingFor(asc).compare(Some(1), Some(2)))
    println(opt.scalaOrderingFor(desc).compare(None, Some(1)))
  }
}
