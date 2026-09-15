import slick.jdbc.MySQLProfile.api._
import slick.collection.heterogeneous._
import slick.collection.heterogeneous.syntax._

case class HRow(id: Int, name: String)

object Main {
  def main(args: Array[String]): Unit = {
    val plain: Int :: String :: HNil = 42 :: "ok" :: HNil
    println(plain.head)
    println(plain.tail.head)
    // Indexing is a macro whose generated casts contain singleton trees.
    println(plain(0))
    println(plain(1))
    val end: _root_.slick.collection.heterogeneous.HNil.type = HNil
    println(end.isEmpty)
    // Shape witnesses live in HList's companion, which is reached through
    // the inferred HCons base class without importing that companion.
    val projection = (LiteralColumn(42) :: LiteralColumn("ok") :: HNil).shaped
    println(projection.toNode.nodeType)
    val mapped = (LiteralColumn(42) :: LiteralColumn("ok") :: HNil).mapTo[HRow]
    println(mapped.toNode.nodeType)
  }
}
