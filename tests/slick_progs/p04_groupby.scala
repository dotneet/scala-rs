// groupBy and aggregates: length / sum / max / min / avg.
import cats.effect.unsafe.implicits.global
import slick.cats.Database
import slick.jdbc.{DatabaseConfig, H2Profile}
import slick.jdbc.H2Profile.api._

object Main {
  class Sales(tag: Tag) extends Table[(Int, String, Int)](tag, "SALES") {
    def id = column[Int]("ID", O.PrimaryKey)
    def region = column[String]("REGION")
    def amount = column[Int]("AMOUNT")
    def * = (id, region, amount)
  }
  val sales = TableQuery[Sales]

  def main(args: Array[String]): Unit = {
    val dc = DatabaseConfig.forURL(H2Profile, "jdbc:h2:mem:p04;DB_CLOSE_DELAY=-1",
      driver = "org.h2.Driver", keepAliveConnection = true)
    val db = Database.make(dc).unsafeRunSync()
    def r[R](a: DBIOAction[R, NoStream, Nothing]): R = db.run(a).unsafeRunSync()
    def show[T, U](label: String, q: Query[T, U, Seq]): Unit = {
      println(label + " sql: " + q.result.statements.mkString("|"))
      println(label + " res: " + r(q.result))
    }
    try {
      r(sales.schema.create)
      r(sales ++= Seq((1, "east", 10), (2, "west", 20), (3, "east", 5), (4, "north", 7), (5, "west", 1)))

      println("count sql: " + sales.length.result.statements.mkString("|"))
      println("count res: " + r(sales.length.result))
      println("sum res:   " + r(sales.map(_.amount).sum.result))
      println("max res:   " + r(sales.map(_.amount).max.result))
      println("min res:   " + r(sales.map(_.amount).min.result))
      println("avg res:   " + r(sales.map(_.amount).avg.result))
      println("empty sum: " + r(sales.filter(_.region === "nowhere").map(_.amount).sum.result))
      println("exists:    " + r(sales.filter(_.amount > 15).exists.result))

      val byRegion = sales.groupBy(_.region).map { case (region, group) =>
        (region, group.length, group.map(_.amount).sum, group.map(_.amount).max)
      }
      show("groupBy", byRegion.sortBy(_._1))

      val having = sales.groupBy(_.region).map { case (region, g) => (region, g.map(_.amount).sum) }
        .filter(_._2 > 10)
      show("having", having.sortBy(_._1))

      val twoKeys = sales.groupBy(s => (s.region, s.amount > 6)).map { case ((rg, big), g) => (rg, big, g.length) }
      show("groupBy2", twoKeys.sortBy(t => (t._1, t._2)))
    } finally db.close()
  }
}
