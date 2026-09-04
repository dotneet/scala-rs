// Column types and schema DDL: Long, Boolean, BigDecimal, Date, autoinc,
// default values, indexes, composite primary keys.
import cats.effect.unsafe.implicits.global
import slick.cats.Database
import slick.jdbc.{DatabaseConfig, H2Profile}
import slick.jdbc.H2Profile.api._
import java.sql.Date

object Main {
  class Items(tag: Tag) extends Table[(Int, String, Long, Boolean, BigDecimal, Date)](tag, "ITEMS") {
    def id = column[Int]("ID", O.PrimaryKey, O.AutoInc)
    def name = column[String]("NAME", O.Length(40))
    def qty = column[Long]("QTY", O.Default(1L))
    def active = column[Boolean]("ACTIVE", O.Default(true))
    def price = column[BigDecimal]("PRICE")
    def made = column[Date]("MADE")
    def * = (id, name, qty, active, price, made)
    def nameIdx = index("ITEMS_NAME_IDX", name, unique = true)
  }
  val items = TableQuery[Items]

  class Pairs(tag: Tag) extends Table[(Int, Int, String)](tag, "PAIRS") {
    def a = column[Int]("A")
    def b = column[Int]("B")
    def label = column[String]("LABEL")
    def * = (a, b, label)
    def pk = primaryKey("PAIRS_PK", (a, b))
  }
  val pairs = TableQuery[Pairs]

  def main(args: Array[String]): Unit = {
    val dc = DatabaseConfig.forURL(H2Profile, "jdbc:h2:mem:p10;DB_CLOSE_DELAY=-1",
      driver = "org.h2.Driver", keepAliveConnection = true)
    val db = Database.make(dc).unsafeRunSync()
    def r[R](a: DBIOAction[R, NoStream, Nothing]): R = db.run(a).unsafeRunSync()
    try {
      (items.schema ++ pairs.schema).createStatements.foreach(println)
      (items.schema ++ pairs.schema).dropStatements.foreach(println)
      r((items.schema ++ pairs.schema).create)
      val d = Date.valueOf("2020-01-02")
      val ins = items.map(i => (i.name, i.price, i.made)) ++= Seq(
        ("nut", BigDecimal("1.50"), d), ("bolt", BigDecimal("2.25"), Date.valueOf("2021-03-04")))
      println("ins=" + r(ins))
      println("all: " + r(items.sortBy(_.id).result))
      println("autoinc back: " + r((items.map(i => (i.name, i.price, i.made))
        returning items.map(_.id)) += (("washer", BigDecimal("0.75"), d))))
      println("all: " + r(items.sortBy(_.id).result))
      println("bd sum: " + r(items.map(_.price).sum.result))
      println("bool:   " + r(items.filter(_.active).length.result))
      println("date:   " + r(items.filter(_.made === d).map(_.name).sortBy(x => x).result))
      println("pairs=" + r(pairs ++= Seq((1, 2, "x"), (1, 3, "y"), (2, 1, "z"))))
      println("pairs: " + r(pairs.sortBy(p => (p.a, p.b)).result))
      println("cast:  " + r(items.sortBy(_.id).map(_.qty.asColumnOf[String]).result))
    } finally db.close()
  }
}
