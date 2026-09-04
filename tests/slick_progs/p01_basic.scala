// Ordinary slick usage: table definition, schema.create, inserts, a select.
// Every program in this directory is compiled ONCE by real scalac and then run
// against both slick builds; anything it prints is compared byte for byte.
import cats.effect.unsafe.implicits.global
import slick.cats.Database
import slick.jdbc.{DatabaseConfig, H2Profile}
import slick.jdbc.H2Profile.api._

object Main {
  class Coffees(tag: Tag) extends Table[(String, Double)](tag, "COFFEES") {
    def name = column[String]("COF_NAME", O.PrimaryKey)
    def price = column[Double]("PRICE")
    def * = (name, price)
  }
  val coffees = TableQuery[Coffees]

  def main(args: Array[String]): Unit = {
    val dc = DatabaseConfig.forURL(H2Profile, "jdbc:h2:mem:p01;DB_CLOSE_DELAY=-1",
      driver = "org.h2.Driver", keepAliveConnection = true)
    val db = Database.make(dc).unsafeRunSync()
    def r[R](a: DBIOAction[R, NoStream, Nothing]): R = db.run(a).unsafeRunSync()
    try {
      coffees.schema.createStatements.foreach(println)
      r(coffees.schema.create)
      println("inserted=" + r(coffees += ("Colombian", 7.99)))
      println("inserted=" + r(coffees ++= Seq(("French_Roast", 8.99), ("Espresso", 9.99))))
      println(coffees.result.statements.mkString("|"))
      r(coffees.result).foreach(println)
      val q = coffees.filter(_.price < 9.0).map(_.name)
      println(q.result.statements.mkString("|"))
      println(r(q.result))
    } finally db.close()
  }
}
