package mism13.jdbc

import mism13.ast.Node

/** slick's `slick.jdbc.JdbcResultConverter`: `DumpInfo` is never named here --
  * it arrives through the inherited member -- so rewriting `copy` to a `new`
  * spelled by *name* looked the class up in this file's scope and reported
  * `not found: type DumpInfo` with no position at all. */
class Column(val name: String, index: Int) extends Node {
  override def getDumpInfo = super.getDumpInfo.copy(mainInfo = "idx=" + index)
}

object Main {
  def main(args: Array[String]): Unit = {
    println(new Column("c", 3).toString)
    println(new Column("d", 7).getDumpInfo.attrInfo.isEmpty)
  }
}
