// Nullable columns: Option[T] columns, null handling, getOrElse, isEmpty, ===.
import cats.effect.unsafe.implicits.global
import slick.cats.Database
import slick.jdbc.{DatabaseConfig, H2Profile}
import slick.jdbc.H2Profile.api._

object Main {
  class Users(tag: Tag) extends Table[(Int, String, Option[String], Option[Int])](tag, "USERS") {
    def id = column[Int]("ID", O.PrimaryKey)
    def name = column[String]("NAME")
    def email = column[Option[String]]("EMAIL")
    def age = column[Option[Int]]("AGE")
    def * = (id, name, email, age)
  }
  val users = TableQuery[Users]

  def main(args: Array[String]): Unit = {
    val dc = DatabaseConfig.forURL(H2Profile, "jdbc:h2:mem:p05;DB_CLOSE_DELAY=-1",
      driver = "org.h2.Driver", keepAliveConnection = true)
    val db = Database.make(dc).unsafeRunSync()
    def r[R](a: DBIOAction[R, NoStream, Nothing]): R = db.run(a).unsafeRunSync()
    def show[T, U](label: String, q: Query[T, U, Seq]): Unit = {
      println(label + " sql: " + q.result.statements.mkString("|"))
      println(label + " res: " + r(q.result))
    }
    try {
      users.schema.createStatements.foreach(println)
      r(users.schema.create)
      r(users ++= Seq(
        (1, "Ann", Some("ann@example.com"), Some(31)),
        (2, "Bob", None, Some(25)),
        (3, "Cid", Some("cid@example.com"), None),
        (4, "Dee", None, None)))
      show("all", users.sortBy(_.id))
      show("isEmpty", users.filter(_.email.isEmpty).sortBy(_.id))
      show("isDefined", users.filter(_.age.isDefined).sortBy(_.id))
      show("getOrElse", users.sortBy(_.id).map(u => (u.name, u.email.getOrElse("<none>"), u.age.getOrElse(-1))))
      show("optEq", users.filter(_.age === Option(25)).sortBy(_.id))
      show("optMap", users.sortBy(_.id).map(u => u.age.map(_ + 100)))
      println("sumOpt: " + r(users.map(_.age).sum.result))
      println("maxOpt: " + r(users.map(_.email).max.result))
      // updating a nullable column to NULL and back
      println("upd1: " + r(users.filter(_.id === 1).map(_.email).update(None)))
      show("afterNull", users.filter(_.id === 1))
      println("upd2: " + r(users.filter(_.id === 4).map(_.age).update(Some(7))))
      show("afterSet", users.filter(_.id === 4))
    } finally db.close()
  }
}
