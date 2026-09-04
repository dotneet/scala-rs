// filter / map / sortBy / take / drop / distinct / union, with the generated SQL.
import cats.effect.unsafe.implicits.global
import slick.cats.Database
import slick.jdbc.{DatabaseConfig, H2Profile}
import slick.jdbc.H2Profile.api._

object Main {
  class People(tag: Tag) extends Table[(Int, String, Int)](tag, "PEOPLE") {
    def id = column[Int]("ID", O.PrimaryKey)
    def name = column[String]("NAME")
    def age = column[Int]("AGE")
    def * = (id, name, age)
  }
  val people = TableQuery[People]

  def main(args: Array[String]): Unit = {
    val dc = DatabaseConfig.forURL(H2Profile, "jdbc:h2:mem:p02;DB_CLOSE_DELAY=-1",
      driver = "org.h2.Driver", keepAliveConnection = true)
    val db = Database.make(dc).unsafeRunSync()
    def r[R](a: DBIOAction[R, NoStream, Nothing]): R = db.run(a).unsafeRunSync()
    def show[T, U](label: String, q: Query[T, U, Seq]): Unit = {
      println(label + " sql: " + q.result.statements.mkString("|"))
      println(label + " res: " + r(q.result))
    }
    try {
      r(people.schema.create)
      r(people ++= Seq((1, "Ann", 31), (2, "Bob", 25), (3, "Cid", 31), (4, "Dee", 40), (5, "Eve", 25)))
      show("all", people.sortBy(_.id))
      show("filter", people.filter(_.age > 28).sortBy(_.id))
      show("map", people.sortBy(_.id).map(p => (p.name, p.age)))
      show("sortDesc", people.sortBy(p => (p.age.desc, p.name.asc)))
      show("take", people.sortBy(_.id).take(2))
      show("drop", people.sortBy(_.id).drop(3))
      show("distinct", people.map(_.age).distinct.sortBy(a => a))
      show("union", people.filter(_.age === 25).union(people.filter(_.age === 40)).sortBy(_.id))
      show("andOr", people.filter(p => p.age === 25 && p.name =!= "Eve" || p.id === 4).sortBy(_.id))
      show("like", people.filter(_.name.like("%e%")).sortBy(_.id))
      show("inSet", people.filter(_.id inSet Set(1, 3)).sortBy(_.id))
      show("expr", people.map(p => (p.age + 1, p.name.length, p.name.toUpperCase)).sortBy(_._1))
    } finally db.close()
  }
}
