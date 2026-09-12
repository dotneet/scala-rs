// The DDL gitbucket's own table definitions generate.
//
// `TableQuery[T].schema.createStatements` walks the `Table` instance slick's
// `TableQuery` macro built and reads every `column[...]` off it, so a wrong
// column name, SQL type, option or order shows up here before any row exists.
import gbrun.Rig

object Main {
  import Rig._
  import Rig.profile.blockingApi._

  def show(name: String, ss: Iterator[String]): Unit = {
    println(s"-- $name")
    ss.toList.sorted.foreach(s => println(s.replaceAll("\\s+", " ").trim))
  }

  def main(args: Array[String]): Unit = {
    show("ACCOUNT", Accounts.schema.createStatements)
    show("REPOSITORY", Repositories.schema.createStatements)
    show("ISSUE", Issues.schema.createStatements)
    show("LABEL", Labels.schema.createStatements)
    show("ISSUE_COMMENT", IssueComments.schema.createStatements)
    show("ALL-DROP", (Accounts.schema ++ Labels.schema).dropStatements)
    println("-- the table names the TableQuery macro produced")
    println(
      List(Accounts, Repositories, Issues, Labels, IssueComments).map(_.baseTableRow.tableName)
    )
  }
}
