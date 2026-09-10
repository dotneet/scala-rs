import slick.jdbc.H2Profile.api._
object Main {
 def main(args: Array[String]): Unit = {
  val n = 7
  val text = "日本語"
  val q = sql"select $n, $text"
  println(q.as[(Int, String)].statements.mkString("|"))
  q.unitPConv((), new slick.jdbc.PositionedParameters(SqlStorageParameters.create()))
  println(sqlu"update x set y=$n".statements.mkString("|"))
  println(sql"select 1".as[Int].statements.mkString("|"))
  val table = "items"
  val literal = sql"select #$table where id=$n"
  println(literal.as[Int].statements.mkString("|"))
  literal.unitPConv((), new slick.jdbc.PositionedParameters(SqlStorageParameters.create()))
 }
}
