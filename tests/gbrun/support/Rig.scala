// Shared by every program in tests/gbrun: the `Profile` instance the clients
// drive gitbucket's tables through, plus the two pieces of setup that are about
// H2 rather than about gitbucket.
//
// `tests/gitbucket_run.sh` compiles this file together with each client, so a
// client is just its `Main`.
package gbrun

import java.util.Date

import com.github.takezoe.slick.blocking.{BlockingH2Driver, BlockingJdbcProfile}
import gitbucket.core.model._

/// gitbucket's own `Profile` with the components the clients use, bound to H2.
///
/// `profile` is a `lazy val` and not a `val`: `Profile.$init$` reads it while
/// building `dateColumnType`, so a strict `val` in the subclass is still null
/// when the trait's initializer runs (`NullPointerException: ... because the
/// return value of gitbucket.core.model.Profile.profile() is null`). gitbucket's
/// own `ProfileProvider` is a `lazy val` for the same reason.
object Rig
    extends Profile
    with AccountComponent
    with RepositoryComponent
    with IssueComponent
    with LabelComponent
    with IssueCommentComponent {
  override lazy val profile: BlockingJdbcProfile = BlockingH2Driver
}

object Support {
  /// gitbucket declares its nullable columns as `column[String]` and lifts them
  /// in the projection (`url.?`), because its real schema comes from liquibase
  /// and not from slick's DDL -- so `schema.create` writes NOT NULL for every
  /// one of them. Relax exactly those, so the `Option` half of the projections
  /// can be exercised in both directions.
  val Nullable: List[(String, String)] = List(
    ("ACCOUNT", "URL"),
    ("ACCOUNT", "LAST_LOGIN_DATE"),
    ("ACCOUNT", "IMAGE"),
    ("ACCOUNT", "DESCRIPTION"),
    ("REPOSITORY", "DESCRIPTION"),
    ("REPOSITORY", "ORIGIN_USER_NAME"),
    ("REPOSITORY", "ORIGIN_REPOSITORY_NAME"),
    ("REPOSITORY", "PARENT_USER_NAME"),
    ("REPOSITORY", "PARENT_REPOSITORY_NAME"),
    ("REPOSITORY", "EXTERNAL_ISSUES_URL"),
    ("REPOSITORY", "EXTERNAL_WIKI_URL"),
    ("ISSUE", "MILESTONE_ID"),
    ("ISSUE", "PRIORITY_ID"),
    ("ISSUE", "CONTENT")
  )

  def relax(conn: java.sql.Connection, tables: Set[String]): Unit = {
    val st = conn.createStatement()
    for ((t, c) <- Nullable if tables.contains(t))
      st.execute(s"""ALTER TABLE "$t" ALTER COLUMN "$c" SET NULL""")
    st.close()
  }

  /// A fixed instant, so every printed date is the same on both sides.
  val D0 = new Date(1600000000000L)
  val D1 = new Date(1600000060000L)

  def account(n: String): Account =
    Account(0L, n, n.capitalize, n + "@example.com", "pw", n == "ada", Some("u/" + n), D0, D0,
      Some(D1), Some("i/" + n), false, false, Some("d/" + n))

  def issue(owner: String, repo: String, id: Int, opener: String, title: String,
            closed: Boolean): Issue =
    Issue(owner, repo, id, opener, None, None, title, Some("body " + id), closed, D0, D1, false)
}
