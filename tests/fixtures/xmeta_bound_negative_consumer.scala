import xmetaexistential.TableQuery

object Main {
  // String is deliberately outside TableQuery's E <: AbstractTable[_] bound.
  val invalid: TableQuery[String] = null
}
