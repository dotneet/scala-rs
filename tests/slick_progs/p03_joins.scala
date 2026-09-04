// for-comprehensions, implicit joins, explicit inner join and left outer join.
import cats.effect.unsafe.implicits.global
import slick.cats.Database
import slick.jdbc.{DatabaseConfig, H2Profile}
import slick.jdbc.H2Profile.api._

object Main {
  class Suppliers(tag: Tag) extends Table[(Int, String)](tag, "SUP") {
    def id = column[Int]("ID", O.PrimaryKey)
    def name = column[String]("NAME")
    def * = (id, name)
  }
  val suppliers = TableQuery[Suppliers]

  class Coffees(tag: Tag) extends Table[(String, Int, Double)](tag, "COF") {
    def name = column[String]("NAME", O.PrimaryKey)
    def supId = column[Int]("SUP_ID")
    def price = column[Double]("PRICE")
    def * = (name, supId, price)
    def supplier = foreignKey("SUP_FK", supId, suppliers)(_.id)
  }
  val coffees = TableQuery[Coffees]

  def main(args: Array[String]): Unit = {
    val dc = DatabaseConfig.forURL(H2Profile, "jdbc:h2:mem:p03;DB_CLOSE_DELAY=-1",
      driver = "org.h2.Driver", keepAliveConnection = true)
    val db = Database.make(dc).unsafeRunSync()
    def r[R](a: DBIOAction[R, NoStream, Nothing]): R = db.run(a).unsafeRunSync()
    def show[T, U](label: String, q: Query[T, U, Seq]): Unit = {
      println(label + " sql: " + q.result.statements.mkString("|"))
      println(label + " res: " + r(q.result))
    }
    try {
      (suppliers.schema ++ coffees.schema).createStatements.foreach(println)
      r((suppliers.schema ++ coffees.schema).create)
      r(suppliers ++= Seq((1, "Acme"), (2, "Superior"), (3, "Unused")))
      r(coffees ++= Seq(("Colombian", 1, 7.99), ("Espresso", 2, 9.99), ("Decaf", 1, 5.49)))

      val implicitJoin = for {
        c <- coffees
        s <- suppliers if c.supId === s.id
      } yield (c.name, s.name)
      show("implicit", implicitJoin.sortBy(_._1))

      val inner = for {
        (c, s) <- coffees join suppliers on (_.supId === _.id)
      } yield (c.name, s.name, c.price)
      show("inner", inner.sortBy(_._1))

      val left = for {
        (s, c) <- suppliers joinLeft coffees on (_.id === _.supId)
      } yield (s.name, c.map(_.name), c.map(_.price))
      show("leftOuter", left.sortBy(t => (t._1, t._2)))

      val nested = for {
        c <- coffees if c.price < 9.0
        s <- c.supplier
      } yield (s.name, c.name)
      show("fk-nav", nested.sortBy(_._2))

      show("subquery", coffees.filter(_.supId in suppliers.filter(_.name === "Acme").map(_.id)).sortBy(_.name))
      println("exists sql: " + coffees.filter(c => suppliers.filter(_.id === c.supId).exists).sortBy(_.name).result.statements.mkString("|"))
    } finally db.close()
  }
}
