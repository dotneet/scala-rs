// Updates, deletes, DBIO composition, and transactionally (commit and rollback).
import cats.effect.unsafe.implicits.global
import slick.cats.Database
import slick.jdbc.{DatabaseConfig, H2Profile}
import slick.jdbc.H2Profile.api._
import scala.util.{Failure, Success, Try}

object Main {
  class Accounts(tag: Tag) extends Table[(Int, String, Int)](tag, "ACCT") {
    def id = column[Int]("ID", O.PrimaryKey)
    def owner = column[String]("OWNER")
    def balance = column[Int]("BALANCE")
    def * = (id, owner, balance)
  }
  val accounts = TableQuery[Accounts]

  def main(args: Array[String]): Unit = {
    val dc = DatabaseConfig.forURL(H2Profile, "jdbc:h2:mem:p06;DB_CLOSE_DELAY=-1",
      driver = "org.h2.Driver", keepAliveConnection = true)
    val db = Database.make(dc).unsafeRunSync()
    def r[R](a: DBIOAction[R, NoStream, Nothing]): R = db.run(a).unsafeRunSync()
    def dump(label: String): Unit = println(label + ": " + r(accounts.sortBy(_.id).result))
    try {
      r(accounts.schema.create)
      r(accounts ++= Seq((1, "Ann", 100), (2, "Bob", 50), (3, "Cid", 0)))
      dump("start")

      val upd = accounts.filter(_.id === 1).map(_.balance)
      println("update sql: " + upd.updateStatement)
      println("updated=" + r(upd.update(120)))
      dump("afterUpdate")

      val updTwo = accounts.filter(_.balance < 60).map(a => (a.owner, a.balance))
      println("updated2=" + r(updTwo.update(("X", 1))))
      dump("afterUpdate2")

      val del = accounts.filter(_.id === 3)
      println("delete sql: " + del.delete.statements.mkString("|"))
      println("deleted=" + r(del.delete))
      dump("afterDelete")

      // committed transaction
      val transfer = (for {
        _ <- accounts.filter(_.id === 1).map(_.balance).update(20)
        _ <- accounts.filter(_.id === 2).map(_.balance).update(200)
        n <- accounts.length.result
      } yield n).transactionally
      println("tx1 rows=" + r(transfer))
      dump("afterTx1")

      // rolled back transaction
      val bad = (for {
        _ <- accounts.filter(_.id === 1).map(_.balance).update(999)
        _ <- DBIO.failed(new RuntimeException("boom"))
      } yield ()).transactionally
      Try(r(bad)) match {
        case Failure(e) => println("tx2 failed: " + e.getMessage)
        case Success(_) => println("tx2 unexpectedly succeeded")
      }
      dump("afterTx2")

      // sequence / DBIO.seq
      println("seq=" + r(DBIO.sequence(Seq(accounts.length.result, accounts.filter(_.balance > 100).length.result))))
    } finally db.close()
  }
}
