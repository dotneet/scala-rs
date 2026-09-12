// Joins, sorts, aggregates and subqueries over gitbucket's tables -- the part of
// a slick program where the query *tree* is assembled from the table instances
// and then compiled to SQL. A projection that lost a column, or a `byRepository`
// whose `&&` bound the wrong operand, shows up in the SQL and in the rows.
import gbrun.{Rig, Support}
import gitbucket.core.model._

object Main {
  import Rig._
  import Rig.profile.blockingApi._
  import Support._

  def main(args: Array[String]): Unit = {
    val db =
      Database.forURL("jdbc:h2:mem:gbrun_joinsort;DB_CLOSE_DELAY=-1", driver = "org.h2.Driver")
    db.withSession { implicit session =>
      (Accounts.schema ++ Issues.schema ++ Labels.schema ++ IssueComments.schema).create
      relax(session.conn, Set("ACCOUNT", "ISSUE"))

      List("ada", "bob", "cy").foreach(n => Accounts.insert(account(n)))
      Issues.insert(issue("ada", "engine", 1, "bob", "overflow", false))
      Issues.insert(issue("ada", "engine", 2, "cy", "underflow", true))
      Issues.insert(issue("ada", "engine", 3, "bob", "off by one", false))
      Issues.insert(issue("bob", "tools", 1, "ada", "typo", false))
      List("bug", "wont-fix", "docs").zipWithIndex.foreach { case (n, i) =>
        Labels.insert(Label("ada", "engine", 0, n, "%06x".format(0x100000 * (i + 1))))
      }
      IssueComments.insert(IssueComment("ada", "engine", 1, 0, "comment", "cy", "me too", D0, D1))
      IssueComments.insert(IssueComment("ada", "engine", 1, 0, "comment", "ada", "fixed", D1, D1))

      println("-- sortBy")
      println(
        Issues.sortBy(i => (i.userName.asc, i.issueId.desc)).map(i => (i.userName, i.issueId)).list
      )
      println(Issues.filter(_.byRepository("ada", "engine")).sortBy(_.title).map(_.title).list)
      println(Accounts.sortBy(_.userName.desc).map(_.userName).list)

      println("-- join (for-comprehension)")
      val j = for {
        i <- Issues if i.byRepository("ada", "engine")
        a <- Accounts if a.userName === i.openedUserName
      } yield (i.issueId, i.title, a.fullName)
      println(j.sortBy(_._1).list)

      println("-- joinLeft")
      val lj = Issues
        .joinLeft(Accounts)
        .on(_.openedUserName === _.userName)
        .filter { case (i, _) => i.userName === "bob".bind }
        .map { case (i, a) => (i.issueId, a.map(_.mailAddress)) }
      println(lj.list)

      println("-- aggregate")
      println("count=" + Issues.length.run)
      println("open=" + Issues.filter(_.closed === false.bind).length.run)
      println(
        "per-repo=" + Issues
          .groupBy(i => (i.userName, i.repositoryName))
          .map { case (k, g) => (k._1, k._2, g.length) }
          .sortBy(r => (r._1, r._2))
          .list
      )
      println("max-id=" + Issues.filter(_.byRepository("ada", "engine")).map(_.issueId).max.run)
      println(
        "comments-per-issue=" + IssueComments
          .groupBy(c => (c.userName, c.repositoryName, c.issueId))
          .map { case (k, g) => (k._3, g.length) }
          .list
      )

      println("-- subquery / in")
      val openers = Issues.filter(_.byRepository("ada", "engine")).map(_.openedUserName)
      println(Accounts.filter(_.userName in openers).sortBy(_.userName).map(_.userName).list)

      println("-- whole rows through the projection")
      println(Issues.filter(_.byRepository("ada", "engine")).sortBy(_.issueId).list.map(_.title))
      println(
        Labels
          .filter(_.byRepository("ada", "engine"))
          .sortBy(_.labelName)
          .list
          .map(l => (l.labelName, l.fontColor))
      )

      println("-- Profile.RichColumn's &&(guard)")
      def q(withClosed: Boolean) =
        Issues.filter(i => i.byRepository("ada", "engine") && (i.closed === false.bind, !withClosed))
      println(q(true).map(_.issueId).list.sorted)
      println(q(false).map(_.issueId).list.sorted)

      println("-- generated SQL")
      println(Issues.filter(_.byRepository("ada", "engine")).map(_.issueId).result.statements.head)
      println(j.sortBy(_._1).result.statements.head)
    }
  }
}
