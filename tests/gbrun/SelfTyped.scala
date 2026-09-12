// A table defined *outside* gitbucket, in a self-typed component, which is the
// shape that used to pass the wrong outer instance.
//
// `trait MyComponent { self: Profile => import profile.api._ ... }` is how every
// one of gitbucket's 30 model components is written. The `Table` subclass it
// declares is an inner class of the component, its `column[...]` calls go through
// `profile.api`, its date column goes through the `MappedColumnType` the
// `Profile` trait's *initializer* built, and the `TableQuery[T]` macro
// instantiates the table with the component's own `this` as the outer. Getting
// that outer wrong compiles, emits, loads and verifies, and then throws
// `ClassCastException` on the first `column` -- so this program is here to
// execute it, from a component gitbucket's own sources never saw.
import java.util.Date

import com.github.takezoe.slick.blocking.{BlockingH2Driver, BlockingJdbcProfile}
import gitbucket.core.model.{AccountComponent, Profile}

case class Note(
  userName: String,
  repositoryName: String,
  noteId: Int,
  body: String,
  written: Date,
  tag: Option[String]
)

/// `gitbucket.core.model.TemplateComponent` is `protected[model]`, so a client
/// cannot mix it in; the two shared columns and the `byRepository` predicate are
/// written out here instead. Everything else is gitbucket's: the `Profile` self
/// type, `profile.api`, and the `java.util.Date` mapping.
trait NoteComponent { self: Profile =>
  import profile.api._
  import self._

  lazy val Notes = TableQuery[Notes]

  class Notes(tag0: Tag) extends Table[Note](tag0, "NOTE") {
    val userName = column[String]("USER_NAME")
    val repositoryName = column[String]("REPOSITORY_NAME")
    val noteId = column[Int]("NOTE_ID", O AutoInc)
    val body = column[String]("BODY")
    val written = column[Date]("WRITTEN")
    val tag = column[String]("TAG")
    def * = (userName, repositoryName, noteId, body, written, tag.?).mapTo[Note]

    def byRepository(owner: String, repository: String) =
      (userName === owner.bind) && (repositoryName === repository.bind)
    def byAccount(owner: String) = this.userName === owner.bind
    def byPrimaryKey(owner: String, repository: String, id: Int) =
      byRepository(owner, repository) && (this.noteId === id.bind)
  }
}

object MyRig extends Profile with NoteComponent with AccountComponent {
  override lazy val profile: BlockingJdbcProfile = BlockingH2Driver
}

object Main {
  import MyRig._
  import MyRig.profile._
  import MyRig.profile.blockingApi._
  import MyRig.dateColumnType
  import gbrun.Support

  def main(args: Array[String]): Unit = {
    println("-- DDL from a client-defined component")
    Notes.schema.createStatements.toList.foreach(s => println(s.replaceAll("\\s+", " ").trim))
    println("table=" + Notes.baseTableRow.tableName)

    val db =
      Database.forURL("jdbc:h2:mem:gbrun_selftyped;DB_CLOSE_DELAY=-1", driver = "org.h2.Driver")
    db.withSession { implicit session =>
      (Notes.schema ++ Accounts.schema).create
      Support.relax(session.conn, Set("ACCOUNT"))
      val st = session.conn.createStatement()
      st.execute("""ALTER TABLE "NOTE" ALTER COLUMN "TAG" SET NULL""")
      st.close()

      Accounts.insert(Support.account("ada"))
      Accounts.insert(Support.account("bob"))
      Notes.insert(Note("ada", "engine", 0, "first", Support.D0, Some("design")))
      Notes.insert(Note("ada", "engine", 0, "second", Support.D1, None))
      Notes.insert(Note("bob", "tools", 0, "third", Support.D0, Some("bug")))

      println("-- rows, through the client's own mapTo")
      for (n <- Notes.sortBy(n => (n.userName, n.noteId)).list)
        println(
          s"${n.userName}/${n.repositoryName}#${n.noteId} ${n.body} ${n.written.getTime} ${n.tag}"
        )

      println("-- the predicates, built from the table instance")
      println(Notes.filter(_.byRepository("ada", "engine")).map(_.body).sortBy(b => b).list)
      println(Notes.filter(_.byAccount("bob")).map(_.body).list)
      println(Notes.filter(_.byPrimaryKey("ada", "engine", 1)).map(_.body).list)

      println("-- a join between the client's table and gitbucket's")
      val j = for {
        n <- Notes
        a <- Accounts if a.userName === n.userName
      } yield (n.body, a.fullName)
      println(j.sortBy(_._1).list)

      println("-- the date mapping the gitbucket Profile trait installed")
      println(Notes.filter(_.written === Support.D1.bind).map(_.body).list)
      println(Notes.sortBy(_.written.desc).map(_.body).list)

      println("-- update and delete")
      Notes.filter(_.byRepository("ada", "engine")).map(_.tag.?).update(Some("reviewed"))
      println(Notes.filter(_.byRepository("ada", "engine")).map(_.tag.?).list)
      println("deleted=" + Notes.filter(_.byAccount("bob")).delete)
      println(Notes.map(_.body).sortBy(b => b).list)
    }
  }
}
