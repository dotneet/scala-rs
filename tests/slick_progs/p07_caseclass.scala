// Case-class mapping with the `<>` operator (explicit apply/unapply pair).
import cats.effect.unsafe.implicits.global
import slick.cats.Database
import slick.jdbc.{DatabaseConfig, H2Profile}
import slick.jdbc.H2Profile.api._

case class User(id: Int, first: String, last: String, age: Option[Int])

object Main {
  class Users(tag: Tag) extends Table[User](tag, "U7") {
    def id = column[Int]("ID", O.PrimaryKey)
    def first = column[String]("FIRST")
    def last = column[String]("LAST")
    def age = column[Option[Int]]("AGE")
    def * = (id, first, last, age) <> (User.tupled, User.unapply)
    def full = (first, last)
  }
  val users = TableQuery[Users]

  def main(args: Array[String]): Unit = {
    val dc = DatabaseConfig.forURL(H2Profile, "jdbc:h2:mem:p07;DB_CLOSE_DELAY=-1",
      driver = "org.h2.Driver", keepAliveConnection = true)
    val db = Database.make(dc).unsafeRunSync()
    def r[R](a: DBIOAction[R, NoStream, Nothing]): R = db.run(a).unsafeRunSync()
    try {
      r(users.schema.create)
      println("ins=" + r(users += User(1, "Ann", "Adams", Some(31))))
      println("ins=" + r(users ++= Seq(User(2, "Bob", "Brown", None), User(3, "Cid", "Clark", Some(19)))))
      println("all sql: " + users.sortBy(_.id).result.statements.mkString("|"))
      r(users.sortBy(_.id).result).foreach(println)
      println("one: " + r(users.filter(_.id === 2).result.head))
      println("opt: " + r(users.filter(_.id === 9).result.headOption))
      println("proj sql: " + users.sortBy(_.id).map(_.full).result.statements.mkString("|"))
      println("proj res: " + r(users.sortBy(_.id).map(_.full).result))
      println("upd=" + r(users.filter(_.id === 3).update(User(3, "Cid", "Clarkson", Some(20)))))
      println("after: " + r(users.filter(_.id === 3).result))
      // a mapped column projection on top of the mapped table
      val q = users.filter(_.age.map(_ > 20).getOrElse(false)).sortBy(_.id)
      println("filt sql: " + q.result.statements.mkString("|"))
      println("filt res: " + r(q.result))
    } finally db.close()
  }
}
