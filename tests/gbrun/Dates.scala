// gitbucket's `MappedColumnType` for `java.util.Date`.
//
// `Profile` declares
//   implicit val dateColumnType: BaseColumnType[java.util.Date] =
//     MappedColumnType.base[java.util.Date, java.sql.Timestamp](
//       d => new java.sql.Timestamp(d.getTime), t => new java.util.Date(t.getTime))
// in the trait's *initializer*, so every date column in gitbucket goes through
// a pair of closures built there. If the mapping were lost, dropped, or built
// against the wrong outer instance, every `REGISTERED_DATE` in the product
// would be wrong -- and nothing before this point executes it.
import java.util.Date

import gbrun.{Rig, Support}
import gitbucket.core.model._

object Main {
  // Exactly the four imports every gitbucket *service* uses
  // (`IssuesService.scala`). The named `dateColumnType` is the load-bearing
  // one: `Rig._` and `Rig.profile._` both offer that name, and two wildcards
  // offering it make every reference ambiguous -- including the implicit
  // search behind `=== mid.bind` on a `Rep[java.util.Date]`. A named import
  // beats a wildcard, which is how gitbucket breaks the same tie.
  import Rig._
  import Rig.profile._
  import Rig.profile.blockingApi._
  import Rig.dateColumnType
  import Support._

  def main(args: Array[String]): Unit = {
    println("-- the mapped column type gitbucket's Profile installed")
    println(Accounts.baseTableRow.registeredDate.toNode.nodeType.toString)

    val db = Database.forURL("jdbc:h2:mem:gbrun_dates;DB_CLOSE_DELAY=-1", driver = "org.h2.Driver")
    db.withSession { implicit session =>
      (Accounts.schema ++ Issues.schema).create
      relax(session.conn, Set("ACCOUNT", "ISSUE"))

      val epoch = new Date(0L)
      val mid = new Date(1234567890123L)
      val late = new Date(2000000000000L)

      Accounts.insert(account("ada").copy(registeredDate = epoch, updatedDate = mid,
        lastLoginDate = Some(late)))
      Accounts.insert(account("bob").copy(registeredDate = mid, updatedDate = late,
        lastLoginDate = None))

      println("-- read back, as millis (timezone-independent)")
      for (a <- Accounts.sortBy(_.userName).list)
        println(s"${a.userName} ${a.registeredDate.getTime} ${a.updatedDate.getTime} " +
          a.lastLoginDate.map(_.getTime))

      println("-- the mapping survives a comparison in SQL")
      println(Accounts.filter(_.registeredDate === mid.bind).map(_.userName).list)
      println(Accounts.filter(_.registeredDate > epoch.bind).map(_.userName).list)
      println(Accounts.filter(_.updatedDate <= mid.bind).map(_.userName).list)
      println(Accounts.sortBy(_.registeredDate.desc).map(_.userName).list)

      println("-- and through a join's ordering")
      Issues.insert(issue("ada", "engine", 1, "ada", "x", false).copy(registeredDate = late))
      Issues.insert(issue("ada", "engine", 2, "bob", "y", false).copy(registeredDate = epoch))
      val j = for {
        i <- Issues
        a <- Accounts if a.userName === i.openedUserName
      } yield (i.issueId, i.registeredDate, a.registeredDate)
      println(j.sortBy(_._2).list.map { case (id, ir, ar) => (id, ir.getTime, ar.getTime) })

      println("-- an explicit update of a mapped column")
      Accounts.filter(_.userName === "bob".bind).map(_.registeredDate).update(late)
      println(Accounts.filter(_.userName === "bob".bind).first.registeredDate.getTime)

      println("-- Option[Date] set to NULL and back")
      Accounts.filter(_.userName === "ada".bind).map(_.lastLoginDate.?).update(None)
      println(Accounts.filter(_.userName === "ada".bind).first.lastLoginDate)
      Accounts.filter(_.userName === "ada".bind).map(_.lastLoginDate).update(mid)
      println(Accounts.filter(_.userName === "ada".bind).first.lastLoginDate.map(_.getTime))

      println("-- the statement the mapping compiles to")
      println(Accounts.filter(_.registeredDate === mid.bind).map(_.userName).result.statements.head)
    }
  }
}
