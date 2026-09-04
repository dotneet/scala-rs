// Plain SQL: the sql / sqlu / tsql-free interpolators, GetResult, SetParameter.
import cats.effect.unsafe.implicits.global
import slick.cats.Database
import slick.jdbc.{DatabaseConfig, GetResult, H2Profile}
import slick.jdbc.H2Profile.api._

case class Row(id: Int, name: String)

object Main {
  implicit val getRow: GetResult[Row] = GetResult(rs => Row(rs.nextInt(), rs.nextString()))

  def main(args: Array[String]): Unit = {
    val dc = DatabaseConfig.forURL(H2Profile, "jdbc:h2:mem:p09;DB_CLOSE_DELAY=-1",
      driver = "org.h2.Driver", keepAliveConnection = true)
    val db = Database.make(dc).unsafeRunSync()
    def r[R](a: DBIOAction[R, NoStream, Nothing]): R = db.run(a).unsafeRunSync()
    try {
      println("ddl=" + r(sqlu"create table T9(id int primary key, name varchar(64))"))
      println("ins=" + r(sqlu"insert into T9 values (1, 'one')"))
      val n = 2
      val s = "two"
      println("ins=" + r(sqlu"insert into T9 values ($n, $s)"))
      println("ins=" + r(sqlu"insert into T9 values (3, 'three')"))
      val q = sql"select id, name from T9 order by id".as[(Int, String)]
      println("sql: " + q.statements.mkString("|"))
      println("res: " + r(q))
      println("mapped: " + r(sql"select id, name from T9 order by id".as[Row]))
      val lo = 2
      println("param: " + r(sql"select name from T9 where id >= $lo order by id".as[String]))
      println("scalar: " + r(sql"select count(*) from T9".as[Int].head))
      println("upd=" + r(sqlu"update T9 set name = 'ONE' where id = 1"))
      println("after: " + r(sql"select name from T9 order by id".as[String]))
    } finally db.close()
  }
}
