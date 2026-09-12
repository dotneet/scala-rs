// Insert and read back gitbucket's own case classes through the `<>` / `mapTo`
// projections its tables declare, against an in-memory H2 database.
//
// This is the one check the compile measures cannot make: `mapTo[Account]` is a
// slick *macro*, and what it expands to is a pair of functions that build an
// `Account` out of a 14-field tuple and take it apart again. A wrong field
// order, a wrong `Option` lift, or a projection that silently drops a column
// all typecheck, emit, load and verify; they only show up when a row goes in and
// comes back. `Repository`'s projection is the `.shaped.<>` form with a nested
// tuple, which is the same question asked a second way.
import gbrun.{Rig, Support}
import gitbucket.core.model._

object Main {
  import Rig._
  import Rig.profile.blockingApi._
  import Support._

  val ada = account("ada").copy(description = Some("first programmer"))
  val bob = account("bob").copy(url = None, lastLoginDate = None, image = None, description = None)

  val bug = issue("ada", "engine", 1, "bob", "it overflows", false).copy(priorityId = Some(3))

  val label = Label("ada", "engine", 0, "bug", "ff0000")

  val comment =
    IssueComment("ada", "engine", 1, 0, "comment", "ada", "reproduced", D0, D1)

  val repo = Repository(
    userName = "ada",
    repositoryName = "engine",
    isPrivate = false,
    description = Some("the difference engine"),
    defaultBranch = "main",
    registeredDate = D0,
    updatedDate = D0,
    lastActivityDate = D1,
    originUserName = None,
    originRepositoryName = None,
    parentUserName = None,
    parentRepositoryName = None,
    options = RepositoryOptions(
      issuesOption = "PUBLIC",
      externalIssuesUrl = None,
      wikiOption = "PUBLIC",
      externalWikiUrl = None,
      allowFork = true,
      mergeOptions = "merge-commit,squash,rebase",
      defaultMergeOption = "merge-commit",
      safeMode = false
    ),
    repositoryId = 0L
  )

  def main(args: Array[String]): Unit = {
    val db =
      Database.forURL("jdbc:h2:mem:gbrun_roundtrip;DB_CLOSE_DELAY=-1", driver = "org.h2.Driver")
    db.withSession { implicit session =>
      (Accounts.schema ++ Repositories.schema ++ Issues.schema ++ Labels.schema ++
        IssueComments.schema).create
      relax(session.conn, Set("ACCOUNT", "REPOSITORY", "ISSUE"))

      Accounts.insert(ada)
      Accounts.insert(bob)
      Repositories.insert(repo)
      Issues.insert(bug)
      Labels.insert(label)
      IssueComments.insert(comment)

      println("accounts=" + Accounts.length.run)

      val back = Accounts.filter(_.userName === "ada".bind).first
      println("read-back: " + back)
      println("equal-ignoring-id: " + (back.copy(accountId = 0L) == ada))
      println("options: " + (back.url, back.image, back.lastLoginDate.map(_.getTime), back.description))

      val bobBack = Accounts.filter(_.userName === "bob".bind).first
      println("bob: " + bobBack.copy(accountId = 0L))
      println("bob nones: " + (bobBack.url, bobBack.image, bobBack.lastLoginDate, bobBack.description))

      // `Repository`'s projection is `.shaped.<>` over a nested tuple, and
      // `RepositoryOptions` is rebuilt by `apply.tupled`.
      val r = Repositories.filter(t => t.byRepository("ada", "engine")).first
      println("repo: " + r.copy(repositoryId = 0L))
      println("repo options: " + r.options)
      println("repo equal: " + (r.copy(repositoryId = 0L) == repo))

      val i = Issues.filter(_.byPrimaryKey("ada", "engine", 1)).first
      println("issue: " + i)
      println("issue equal: " + (i == bug))
      println("issue options: " + (i.milestoneId, i.priorityId, i.content))

      val l = Labels.filter(_.byRepository("ada", "engine")).first
      println("label: " + l.copy(labelId = 0))
      println("label fontColor: " + l.fontColor)

      val c = IssueComments.filter(_.byIssue("ada", "engine", 1)).first
      println("comment: " + c.copy(commentId = 0))
      println("comment equal: " + (c.copy(commentId = 0) == comment))

      // update one column, then a whole row through the projection's
      // *unpacking* direction
      Accounts.filter(_.userName === "ada".bind).map(_.fullName).update("Augusta Ada King")
      println("updated: " + Accounts.filter(_.userName === "ada".bind).first.fullName)
      Accounts.filter(_.userName === "bob".bind).update(bobBack.copy(fullName = "Robert"))
      println("row-update: " + Accounts.filter(_.userName === "bob".bind).first.fullName)

      println("list: " + Accounts.sortBy(_.userName).map(_.userName).list)
      Accounts.filter(_.userName === "bob".bind).delete
      println("after delete: " + Accounts.map(_.userName).list)
    }
  }
}
