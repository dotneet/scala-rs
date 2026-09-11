// slick's `(a, b, c).mapTo[Row]` against row classes this run is compiling,
// the way every real slick schema is written: the case class and the table
// in the same compilation. `docs/macros.md` §7.25.
//
// `ShapedValue.mapToImpl` interrogates its type argument -- is it a case
// class, what is its companion, does the companion have `tupled`, what are
// the case accessor fields and their types -- and builds a tree whose shape
// depends on every answer. The four row classes below take each of its
// branches: a companion with `tupled` (`Account`), a one-field class
// (`Tag1`: `apply`/`unapply` directly), an explicit companion without
// `tupled` (`Label`: `(apply _).tupled`), and a class implementing a trait's
// abstract `val`s, with a default argument and `Option` and timestamp
// fields (`Comment`, the shape of gitbucket's `IssueComment`). Two `mapTo`s
// sit in one table class, and the tables live in a trait with a self type
// and an imported profile API, as gitbucket's do.
//
// Real scalac 2.13.16 compiles this file; `crates/cli/tests/gbmac.rs` runs
// both builds against an in-memory H2 database under `-Xverify:all` and
// requires the same output. The rows go in through the mapper's `toBase`
// and come back through its fast path.
//
// The tables extend `profile.Table` rather than the `Table` the imported API
// aliases. Written through the alias, scala-rs passes the component
// (`Tables.this`) where the profile belongs as the superclass's outer
// instance, and the constructor fails with a `ClassCastException` at run
// time -- a defect independent of `mapTo`, recorded in `docs/macros.md`
// §7.25 with its own reproduction.
package gbmacm

import scala.concurrent.{Await, ExecutionContext, Future}
import scala.concurrent.duration._

trait Stamped {
  val note: String
  val at: java.sql.Timestamp
}

case class Account(userName: String, age: Int, email: Option[String])
case class Tag1(value: String)
case class Label(id: Int, name: String)
object Label {
  def blank: Label = Label(0, "")
}
case class Comment(id: Int = 0, note: String, at: java.sql.Timestamp, parent: Option[Int]) extends Stamped

trait Profile {
  val profile: slick.jdbc.JdbcProfile
}

trait Tables { self: Profile =>
  import profile.api._

  class Accounts(tag: Tag) extends profile.Table[Account](tag, "ACCOUNT") {
    val userName = column[String]("USER_NAME", O.PrimaryKey)
    val age = column[Int]("AGE")
    val email = column[Option[String]]("EMAIL")
    def * = (userName, age, email).mapTo[Account]
    def byName = (userName, age, email).mapTo[Account]
  }

  class Tags(tag: Tag) extends profile.Table[Tag1](tag, "TAGS") {
    val value = column[String]("VALUE")
    def * = (value).mapTo[Tag1]
  }

  class Labels(tag: Tag) extends profile.Table[Label](tag, "LABELS") {
    val id = column[Int]("ID")
    val name = column[String]("NAME")
    def * = (id, name).mapTo[Label]
  }

  class Comments(tag: Tag) extends profile.Table[Comment](tag, "COMMENTS") {
    val id = column[Int]("ID")
    val note = column[String]("NOTE")
    val at = column[java.sql.Timestamp]("AT")
    val parent = column[Option[Int]]("PARENT")
    def * = (id, note, at, parent).mapTo[Comment]
  }

  lazy val Accounts = TableQuery[Accounts]
  lazy val Tags = TableQuery[Tags]
  lazy val Labels = TableQuery[Labels]
  lazy val Comments = TableQuery[Comments]
}

object Db extends Tables with Profile {
  val profile: slick.jdbc.JdbcProfile = slick.jdbc.H2Profile
}

object Main {
  import Db._
  import Db.profile.api._

  implicit val ec: ExecutionContext = ExecutionContext.global

  def await[T](f: Future[T]): T = Await.result(f, 30.seconds)

  def main(args: Array[String]): Unit = {
    val db = slick.jdbc.JdbcBackend.Database.forURL("jdbc:h2:mem:gbmac;DB_CLOSE_DELAY=-1", driver = "org.h2.Driver")
    try {
      val when = new java.sql.Timestamp(86400000L)
      await(db.run(Accounts.schema.create))
      await(db.run(Tags.schema.create))
      await(db.run(Labels.schema.create))
      await(db.run(Comments.schema.create))
      await(db.run(Accounts += Account("alice", 30, Some("a@example.com"))))
      await(db.run(Accounts += Account("bob", 25, None)))
      await(db.run(Tags += Tag1("red")))
      await(db.run(Labels += Label(7, "bug")))
      await(db.run(Comments += Comment(note = "first", at = when, parent = None)))
      await(db.run(Comments += Comment(2, "second", when, Some(1))))
      val accounts: Seq[Account] = await(db.run(Accounts.sortBy(_.userName).result.map(x => x)))
      println(accounts)
      val adults: Seq[String] = await(db.run(Accounts.filter(_.age > 26).map(_.userName).result.map(x => x)))
      println(adults)
      val tags: Seq[Tag1] = await(db.run(Tags.result.map(x => x)))
      println(tags)
      val labels: Seq[Label] = await(db.run(Labels.result.map(x => x)))
      println(labels)
      val comments: Seq[Comment] = await(db.run(Comments.sortBy(_.id).result.map(x => x)))
      println(comments.map(c => (c.id, c.note, c.at.getTime, c.parent)))
      println(comments.map(_.note).mkString(","))
      val stamped: Seq[Stamped] = comments
      println(stamped.map(_.note).mkString(","))
      // The mapping itself, and the fast-path converter the expansion builds
      // as an anonymous subclass of `SimpleFastPathResultConverter`.
      Accounts.baseTableRow.*.toNode match {
        case slick.ast.TypeMapping(_, mapper, ct) =>
          println(mapper.toMapped(("carol", 41, Some("c@example.com"))))
          println(mapper.toBase(Account("dave", 52, None)))
          println(ct)
          val fast = mapper.fastPath.get(converter(3)).asInstanceOf[slick.relational.ResultConverter[_, _]]
          println(fast.getDumpInfo.name)
        case other => println(other)
      }
      Comments.baseTableRow.*.toNode match {
        case slick.ast.TypeMapping(_, mapper, _) =>
          val fast = mapper.fastPath.get(converter(4)).asInstanceOf[slick.relational.ResultConverter[_, _]]
          println(fast.getDumpInfo.name)
        case other => println(other)
      }
    } finally db.close()
  }

  import slick.relational.{ProductResultConverter, ResultConverter, ResultConverterDomain, TypeMappingResultConverter}

  def element: ResultConverter[ResultConverterDomain, Any] = new ResultConverter[ResultConverterDomain, Any] {
    def read(pr: Reader): Any = null
    def update(value: Any, pr: Updater): Unit = ()
    def set(value: Any, pp: Writer): Unit = ()
    def width = 1
  }

  def converter(n: Int): TypeMappingResultConverter[ResultConverterDomain, Any, Product] =
    TypeMappingResultConverter[ResultConverterDomain, Any, Product](
      ProductResultConverter[ResultConverterDomain, Product](Seq.fill(n)(element): _*),
      (x: Any) => x.asInstanceOf[Product],
      (p: Product) => p)
}
