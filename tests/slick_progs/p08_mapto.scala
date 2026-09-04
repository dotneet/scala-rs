// The `mapTo` macro. It is a blackbox macro living in slick's own classfiles,
// so this program exercises the macro implementation that scala-rs compiled --
// but only when MODE=a puts the scala-rs build on the *compile* classpath.
// In the default MODE=b it still checks that the expansion's runtime helpers
// (MappedProjection, MappedScalaType.Mapper) behave the same in both builds.
import cats.effect.unsafe.implicits.global
import slick.cats.Database
import slick.jdbc.{DatabaseConfig, H2Profile}
import slick.jdbc.H2Profile.api._

case class Book(id: Int, title: String, pages: Int)
case class Tagged(value: String, n: Int)

object Main {
  class Books(tag: Tag) extends Table[Book](tag, "BOOKS") {
    def id = column[Int]("ID", O.PrimaryKey)
    def title = column[String]("TITLE")
    def pages = column[Int]("PAGES")
    def * = (id, title, pages).mapTo[Book]
  }
  val books = TableQuery[Books]

  class Labels(tag: Tag) extends Table[Tagged](tag, "LABELS") {
    def value = column[String]("VALUE", O.PrimaryKey)
    def n = column[Int]("N")
    def * = (value, n).mapTo[Tagged]
  }
  val labels = TableQuery[Labels]

  def main(args: Array[String]): Unit = {
    val dc = DatabaseConfig.forURL(H2Profile, "jdbc:h2:mem:p08;DB_CLOSE_DELAY=-1",
      driver = "org.h2.Driver", keepAliveConnection = true)
    val db = Database.make(dc).unsafeRunSync()
    def r[R](a: DBIOAction[R, NoStream, Nothing]): R = db.run(a).unsafeRunSync()
    try {
      r((books.schema ++ labels.schema).create)
      println("ins=" + r(books ++= Seq(Book(1, "Dune", 412), Book(2, "Ubik", 224), Book(3, "Solaris", 204))))
      println("sql: " + books.sortBy(_.id).result.statements.mkString("|"))
      r(books.sortBy(_.id).result).foreach(println)
      println("filter: " + r(books.filter(_.pages > 210).sortBy(_.id).result))
      println("proj:   " + r(books.sortBy(_.id).map(b => (b.title, b.pages)).result))
      println("upd=" + r(books.filter(_.id === 2).update(Book(2, "Ubik!", 225))))
      println("after:  " + r(books.filter(_.id === 2).result))
      println("ins=" + r(labels ++= Seq(Tagged("a", 1), Tagged("b", 2))))
      println("labels: " + r(labels.sortBy(_.value).result))
    } finally db.close()
  }
}
