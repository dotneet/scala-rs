// MappedColumnType (a user-defined column type), Compiled parameterised
// queries, and streaming a query with fs2.
import cats.effect.IO
import cats.effect.unsafe.implicits.global
import slick.cats.Database
import slick.jdbc.{DatabaseConfig, H2Profile}
import slick.jdbc.H2Profile.api._

sealed trait Colour
case object Red extends Colour
case object Green extends Colour
case object Blue extends Colour

object Main {
  implicit val colourType: BaseColumnType[Colour] =
    MappedColumnType.base[Colour, String](
      { case Red => "R"; case Green => "G"; case Blue => "B" },
      { case "R" => Red; case "G" => Green; case "B" => Blue })

  class Bricks(tag: Tag) extends Table[(Int, Colour, Option[Colour])](tag, "BRICKS") {
    def id = column[Int]("ID", O.PrimaryKey)
    def c = column[Colour]("C")
    def alt = column[Option[Colour]]("ALT")
    def * = (id, c, alt)
  }
  val bricks = TableQuery[Bricks]

  val byColour = Compiled((c: Rep[Colour]) => bricks.filter(_.c === c).sortBy(_.id))
  val byIdRange = Compiled((lo: Rep[Int], hi: Rep[Int]) => bricks.filter(b => b.id >= lo && b.id <= hi).sortBy(_.id))

  def main(args: Array[String]): Unit = {
    val dc = DatabaseConfig.forURL(H2Profile, "jdbc:h2:mem:p12;DB_CLOSE_DELAY=-1",
      driver = "org.h2.Driver", keepAliveConnection = true)
    val db = Database.make(dc).unsafeRunSync()
    def r[R](a: DBIOAction[R, NoStream, Nothing]): R = db.run(a).unsafeRunSync()
    try {
      bricks.schema.createStatements.foreach(println)
      r(bricks.schema.create)
      println("ins=" + r(bricks ++= Seq((1, Red, Some(Blue)), (2, Green, None), (3, Red, Some(Red)))))
      println("all sql: " + bricks.sortBy(_.id).result.statements.mkString("|"))
      println("all: " + r(bricks.sortBy(_.id).result))
      println("byColour sql: " + byColour(Red).result.statements.mkString("|"))
      println("byColour: " + r(byColour(Red).result))
      println("byRange: " + r(byIdRange(2, 3).result))
      println("eq: " + r(bricks.filter(_.c === (Blue: Colour)).length.result))
      println("optCol: " + r(bricks.sortBy(_.id).map(_.alt).result))
      val streamed = db.stream(bricks.sortBy(_.id).map(_.id).result).compile.toList.unsafeRunSync()
      println("stream: " + streamed)
      val io: IO[Int] = db.run(bricks.length.result)
      println("io: " + io.unsafeRunSync())
    } finally db.close()
  }
}
