// SQL generation only: no database is touched. This is the widest probe of
// slick's query compiler (the bulk of slick's code), and a miscompiled
// optimizer phase shows up here as a different SQL string.
import slick.jdbc.H2Profile.api._

object Main {
  class A(tag: Tag) extends Table[(Int, String, Option[Int])](tag, "A") {
    def id = column[Int]("ID", O.PrimaryKey)
    def s = column[String]("S")
    def o = column[Option[Int]]("O")
    def * = (id, s, o)
  }
  val as = TableQuery[A]
  class B(tag: Tag) extends Table[(Int, Int)](tag, "B") {
    def id = column[Int]("ID", O.PrimaryKey)
    def aid = column[Int]("AID")
    def * = (id, aid)
  }
  val bs = TableQuery[B]

  def p(label: String, ss: Iterable[String]): Unit = println(label + ": " + ss.mkString("|"))

  def main(args: Array[String]): Unit = {
    p("plain", as.result.statements)
    p("filter", as.filter(_.id > 3).result.statements)
    p("map", as.map(a => (a.s, a.id * 2)).result.statements)
    p("sort", as.sortBy(a => (a.s.desc, a.id)).result.statements)
    p("take", as.sortBy(_.id).drop(2).take(3).result.statements)
    p("distinct", as.map(_.s).distinct.result.statements)
    p("count", as.length.result.statements)
    p("group", as.groupBy(_.s).map { case (s, g) => (s, g.length, g.map(_.id).sum) }.result.statements)
    p("join", (as join bs on (_.id === _.aid)).map { case (a, b) => (a.s, b.id) }.result.statements)
    p("leftJoin", (as joinLeft bs on (_.id === _.aid)).map { case (a, b) => (a.s, b.map(_.id)) }.result.statements)
    p("rightJoin", (as joinRight bs on (_.id === _.aid)).map { case (a, b) => (a.map(_.s), b.id) }.result.statements)
    p("fullJoin", (as joinFull bs on (_.id === _.aid)).map { case (a, b) => (a.map(_.s), b.map(_.id)) }.result.statements)
    p("cross", (as join bs).map { case (a, b) => (a.id, b.id) }.result.statements)
    p("union", as.filter(_.id === 1).union(as.filter(_.id === 2)).result.statements)
    p("unionAll", as.filter(_.id === 1).unionAll(as.filter(_.id === 2)).result.statements)
    p("in", as.filter(_.id in bs.map(_.aid)).result.statements)
    p("exists", as.filter(a => bs.filter(_.aid === a.id).exists).result.statements)
    p("nested", as.filter(_.id > 1).sortBy(_.s).take(5).filter(_.id < 100).result.statements)
    p("forcompr", (for { a <- as; b <- bs if b.aid === a.id; if a.id > 0 } yield (a.s, b.id)).result.statements)
    p("optMap", as.map(_.o.map(_ + 1)).result.statements)
    p("case", as.map(a => Case.If(a.id > 3).Then("big").Else("small")).result.statements)
    p("str", as.map(a => (a.s ++ "x", a.s.length, a.s.toUpperCase, a.s.trim)).result.statements)
    p("math", as.map(a => (a.id + 1, a.id - 1, a.id * 2, a.id / 2, a.id % 2)).result.statements)
    p("update", Seq(as.filter(_.id === 1).map(_.s).updateStatement))
    p("delete", as.filter(_.id === 1).delete.statements)
    p("insert", Seq(as.insertStatement))
    p("ddl", as.schema.createStatements.toSeq ++ bs.schema.createStatements.toSeq)
    p("subselect", as.map(a => (a.id, bs.filter(_.aid === a.id).length)).result.statements)
    p("sortNulls", as.sortBy(_.o.desc.nullsFirst).result.statements)
    p("zipWithIndex", as.sortBy(_.id).zipWithIndex.map { case (a, i) => (a.id, i) }.result.statements)
  }
}
